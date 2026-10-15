use super::*;
#[derive(Clone)]
pub(super) struct Session {
    pub started: Instant,
    pub active_ms: u64,
    active_since: Option<Instant>,
    pub starts: u64,
    pub completed: u64,
    pub skipped: u64,
    pub failed: u64,
    pub requesters: HashMap<u64, u64>,
    pub recent: VecDeque<String>,
}
impl Session {
    pub fn new(at: Instant) -> Self {
        Self {
            started: at,
            active_ms: 0,
            active_since: None,
            starts: 0,
            completed: 0,
            skipped: 0,
            failed: 0,
            requesters: HashMap::new(),
            recent: VecDeque::new(),
        }
    }
    pub fn active(&mut self, active: bool, at: Instant) {
        if let Some(start) = self.active_since.take() {
            self.active_ms = self.active_ms.saturating_add(
                at.saturating_duration_since(start)
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
            );
        }
        if active {
            self.active_since = Some(at);
        }
    }
    pub fn end_track(&mut self, natural: bool, skipped: bool, at: Instant) {
        self.active(false, at);
        if natural {
            self.completed = self.completed.saturating_add(1);
        } else if skipped {
            self.skipped = self.skipped.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
        }
    }
    pub fn start_track(&mut self, request: &Request, at: Instant) {
        self.starts = self.starts.saturating_add(1);
        if self.requesters.len() < 1000 || self.requesters.contains_key(&request.user) {
            let n = self.requesters.entry(request.user).or_default();
            *n = n.saturating_add(1);
        }
        self.recent.push_back(safe(&request.track.info.title, 90));
        while self.recent.len() > 5 {
            self.recent.pop_front();
        }
        self.active(true, at);
    }
}
pub(super) async fn send(
    node: &Service,
    ctx: &s::Context,
    session: Session,
    channel: Option<s::ChannelId>,
    reason: &str,
) {
    let Some(channel) = channel else {
        return;
    };
    let result:Result<(),Error>=async{
        let cfg=crate::store::settings(&node.db,node.guild.get()).await?;
        if cfg.music_recaps_disabled || session.starts==0{return Ok(());}
        let fresh=channel.to_channel(&ctx.http).await?.guild().ok_or_else(||crate::errors::user("Recap channel unavailable"))?;
        if fresh.guild_id!=node.guild{return Ok(());}
        let hidden=sqlx::query_scalar::<_,String>("SELECT user FROM activity_optout WHERE guild=?").bind(node.guild.to_string()).fetch_all(&node.db).await?.into_iter().filter_map(|v|v.parse::<u64>().ok()).collect::<HashSet<_>>();
        let mut leaders=session.requesters.iter().filter(|(id,_)|!hidden.contains(id)).map(|(id,n)|(*id,*n)).collect::<Vec<_>>();leaders.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));
        let top=leaders.iter().take(5).enumerate().map(|(i,(id,n))|format!("`{:02}` <@{id}> · {n} track start(s)",i+1)).collect::<Vec<_>>();
        let recent=session.recent.iter().map(|s|format!("• {s}")).collect::<Vec<_>>().join("\n");
        let png=crate::cards::render("MUSIC / SESSION RECAP","That was a good session.","Same room. Shared soundtrack.",&[("TRACK STARTS",session.starts.to_string()),("ACTIVE TIME",time(session.active_ms)),("COMPLETED",session.completed.to_string()),("SKIPPED",session.skipped.to_string()),("FAILED",session.failed.to_string()),("SESSION",time(session.started.elapsed().as_millis().min(u64::MAX as u128) as u64))],None)?;
        let embed=crate::emoji::embed_for("headphones").title("ELVNX / SESSION RECAP").description(format!("**{reason}**\nThanks for sharing the soundtrack.\n\n**{}** track start(s) · **{}** completed · **{}** skipped · **{}** failed\nEstimated active playback: **{}** (paused time excluded).",session.starts,session.completed,session.skipped,session.failed,time(session.active_ms))).field("Top requesters",if top.is_empty(){"Individual rankings are hidden by privacy choices.".into()}else{top.join("\n")},false).field("Last in the rotation",recent,false).image("attachment://session-recap.png").footer(s::CreateEmbedFooter::new("11X / avrxt.dev • session totals only • no audio recorded"));
        channel.send_message(&ctx.http,s::CreateMessage::new().embed(embed).add_file(s::CreateAttachment::bytes(png,"session-recap.png")).allowed_mentions(s::CreateAllowedMentions::new())).await?;Ok(())
    }.await;
    if let Err(error) = result {
        tracing::warn!(%error,"Music session recap delivery failed");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recaps_count_starts_separately_from_completed_skipped_and_failed() {
        let at = Instant::now();
        let track:Track=serde_json::from_value(json!({"encoded":"x","info":{"title":"Track","author":"A","length":10000,"isStream":false,"isSeekable":true,"uri":null}})).unwrap();
        let r = Request {
            track,
            user: 7,
            id: 1,
        };
        let mut s = Session::new(at);
        for i in 0..8 {
            s.start_track(&r, at + Duration::from_secs(i * 10));
            s.end_track(i < 6, i == 6, at + Duration::from_secs(i * 10 + 5));
        }
        assert_eq!(s.starts, 8);
        assert_eq!(s.completed, 6);
        assert_eq!(s.skipped, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.active_ms, 40000);
        assert_eq!(s.recent.len(), 5);
        assert_eq!(s.requesters.get(&7), Some(&8));
    }
    #[test]
    fn playback_clock_excludes_pauses_and_is_not_inflated_by_seeks() {
        let t = Instant::now();
        let mut s = Session::new(t);
        s.active(true, t);
        s.active(false, t + Duration::from_secs(10));
        s.active(false, t + Duration::from_secs(40));
        assert_eq!(s.active_ms, 10000);
        s.active(true, t + Duration::from_secs(50));
        s.active(false, t + Duration::from_secs(55));
        assert_eq!(s.active_ms, 15000);
        s.active(false, t + Duration::from_secs(60));
        assert_eq!(s.active_ms, 15000);
    }
}
