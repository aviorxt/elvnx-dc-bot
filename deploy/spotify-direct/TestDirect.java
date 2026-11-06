import online.elvnx.spotify.SpotifyDirectPlugin;
import com.sedmelluq.discord.lavaplayer.player.*;
import com.sedmelluq.discord.lavaplayer.track.*;
import java.util.*;
public class TestDirect {
 public static void main(String[]args)throws Exception{
  var manager=new DefaultAudioPlayerManager();new SpotifyDirectPlugin().configure(manager);
  String json="{\"id\":\"4cOdK2wGLETKBW3PvgPWqT\",\"title\":\"Never Gonna Give You Up\",\"artist\":\"Rick Astley\",\"duration\":213573}";
  var track=(AudioTrack)manager.loadItemSync("spdirect:"+Base64.getUrlEncoder().withoutPadding().encodeToString(json.getBytes()));
  if(track==null||track.isSeekable())throw new AssertionError("Invalid direct track");
  var player=manager.createPlayer();player.playTrack(track);int frames=0;long deadline=System.currentTimeMillis()+30000;
  while(frames<150&&System.currentTimeMillis()<deadline){var frame=player.provide();if(frame!=null&&!frame.isTerminator())frames++;Thread.sleep(20);}
  var http=java.net.http.HttpClient.newHttpClient();
  http.send(java.net.http.HttpRequest.newBuilder(java.net.URI.create("http://127.0.0.1:3680/pause")).POST(java.net.http.HttpRequest.BodyPublishers.noBody()).build(),java.net.http.HttpResponse.BodyHandlers.discarding());
  player.setPaused(true);Thread.sleep(400);
  String status=http.send(java.net.http.HttpRequest.newBuilder(java.net.URI.create("http://127.0.0.1:3680/health")).GET().build(),java.net.http.HttpResponse.BodyHandlers.ofString()).body();
  if(!status.contains("Paused"))throw new AssertionError("Native pause failed");
  http.send(java.net.http.HttpRequest.newBuilder(java.net.URI.create("http://127.0.0.1:3680/resume")).POST(java.net.http.HttpRequest.BodyPublishers.noBody()).build(),java.net.http.HttpResponse.BodyHandlers.discarding());
  player.setPaused(false);
  player.stopTrack();manager.shutdown();System.out.println("Direct Spotify Opus audio frames: "+frames+"; native pause/resume passed");
  if(frames<150)throw new AssertionError("No direct decoded audio");
 }
}
