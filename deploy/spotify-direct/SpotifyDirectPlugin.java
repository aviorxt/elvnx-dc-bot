package online.elvnx.spotify;
import com.sedmelluq.discord.lavaplayer.player.*;
import com.sedmelluq.discord.lavaplayer.source.*;
import com.sedmelluq.discord.lavaplayer.track.*;
import com.sedmelluq.discord.lavaplayer.track.playback.*;
import com.sedmelluq.discord.lavaplayer.filter.*;
import dev.arbjerg.lavalink.api.AudioPlayerManagerConfiguration;
import org.springframework.stereotype.Service;
import com.fasterxml.jackson.databind.*;
import java.io.*; import java.net.*; import java.net.http.*; import java.nio.*; import java.time.Duration; import java.util.*;
@Service
public class SpotifyDirectPlugin implements AudioPlayerManagerConfiguration {
 public AudioPlayerManager configure(AudioPlayerManager m){m.registerSourceManager(new Source());return m;}
 public static class Source implements AudioSourceManager {
  public String getSourceName(){return "spotifydirect";}
  public AudioItem loadItem(AudioPlayerManager m,AudioReference r){
   if(!r.identifier.startsWith("spdirect:"))return null;
   try{JsonNode d=new ObjectMapper().readTree(Base64.getUrlDecoder().decode(r.identifier.substring(9)));
    String id=d.path("id").asText();long length=d.path("duration").asLong();
    if(!id.matches("[A-Za-z0-9]{22}")||length<1||length>14400000)throw new IllegalArgumentException("Invalid Spotify track");
    return new Track(new AudioTrackInfo(d.path("title").asText(),d.path("artist").asText(),length,id,false,"https://open.spotify.com/track/"+id),this);
   }catch(Exception e){throw new RuntimeException("Invalid direct Spotify metadata",e);}
  }
  public boolean isTrackEncodable(AudioTrack t){return true;}
  public void encodeTrack(AudioTrack t,DataOutput o){}
  public AudioTrack decodeTrack(AudioTrackInfo i,DataInput in){return new Track(i,this);}
  public void shutdown(){}
 }
 public static class Track extends BaseAudioTrack {
  final Source source; volatile InputStream input;
  static final HttpClient HTTP=HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(10)).build();
  Track(AudioTrackInfo i,Source s){super(i);source=s;}
  public AudioSourceManager getSourceManager(){return source;}
  public boolean isSeekable(){return false;}
  protected AudioTrack makeShallowClone(){return new Track(trackInfo,source);}
  public void stop(){try{if(input!=null)input.close();}catch(Exception ignored){}super.stop();}
  public void process(LocalAudioTrackExecutor executor)throws Exception {
   String session=UUID.randomUUID().toString();
   AudioPipeline pipe=AudioPipelineFactory.create(executor.getProcessingContext(),new PcmFormat(2,48000));
   try {
    var response=HTTP.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:3680/audio/"+trackInfo.identifier+"?duration="+trackInfo.length+"&session="+session)).GET().build(),HttpResponse.BodyHandlers.ofInputStream());
    if(response.statusCode()!=200){response.body().close();throw new IOException("Official Spotify bridge unavailable: "+response.statusCode());}
    input=response.body();
    executor.executeProcessingLoop(()->{
     byte[] bytes=new byte[3840];int n;
     while((n=input.readNBytes(bytes,0,bytes.length))>0){
      short[] samples=new short[n/2];ByteBuffer.wrap(bytes,0,n).order(ByteOrder.LITTLE_ENDIAN).asShortBuffer().get(samples);
      pipe.process(samples,0,samples.length);
     }
     pipe.flush();
    },null);
   }finally{
    if(input!=null)input.close();pipe.close();
    boolean interrupted=Thread.interrupted();
    try{HTTP.send(HttpRequest.newBuilder(URI.create("http://127.0.0.1:3680/stop?session="+session)).timeout(Duration.ofSeconds(6)).POST(HttpRequest.BodyPublishers.noBody()).build(),HttpResponse.BodyHandlers.discarding());}catch(Exception ignored){}
    if(interrupted)Thread.currentThread().interrupt();
   }
  }
 }
}
