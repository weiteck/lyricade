use anyhow::anyhow;
use tracing::warn;

use crate::{
  Result,
  lyrics::{Lyrics, LyricsType},
};

/// Attempt to parse Timed Text Markup Language (TTML) to LRC lyrics.
pub fn parse_ttml_as_lrc(ttml: &str) -> Result<Lyrics> {
  let doc = roxmltree::Document::parse(ttml)?;

  let Some(tt) = doc.descendants().find(|el| el.has_tag_name("tt")) else {
    warn!("Failed to parse TTML to LRC lyrics: no <tt> element found");
    return Err(anyhow!("Failed to parse TTML to LRC lyrics: no <tt> element found"));
  };

  let Some(body) = tt.descendants().find(|el| el.has_tag_name("body")) else {
    warn!("Failed to parse TTML to LRC lyrics: no <body> element found");
    return Err(anyhow!("Failed to parse TTML to LRC lyrics: no <body> element found"));
  };

  let mut buf = String::new();
  let mut section_added = false;
  let mut lyrics_type = LyricsType::Plain;

  for div in body.descendants().filter(|el| el.has_tag_name("div")) {
    if section_added {
      // Add a blank line between sections, i.e. a <div> element
      buf.push('\n');
    }

    for p in div.descendants().filter(|el| el.has_tag_name("p")) {
      if let Some(ts) = p.attribute("begin") {
        // Add timestamp from "begin" attribute value
        let ts = ts.trim_start_matches(r#"\""#).trim_end_matches(r#"\""#);
        let ts = parse_ttml_timestamp_as_lrc_timestamp(ts)
          .inspect_err(|e| warn!("Failed to parse LRC timestamp from TTML timestamp: {e}"))?;
        buf.push_str(&ts);

        // We know TTML contains timestamps, so lyrics are sync
        lyrics_type = LyricsType::Sync;
      }

      // Add the text within <p> and contained <span>s
      p.descendants()
        .filter(roxmltree::Node::is_text)
        .filter_map(|n| n.text())
        .for_each(|s| buf.push_str(s));

      // Add new line at end of line of lyrics, i.e. a <p> element
      buf.push('\n');
    }

    section_added = true;
  }

  if !buf.is_empty() {
    let lyrics = Lyrics {
      lyrics_type,
      contents: buf,
    };

    return Ok(lyrics);
  }

  warn!("Failed to parse TTML to LRC lyrics");
  Err(anyhow!("Failed to parse TTML to LRC lyrics"))
}

/// Attempt to parse "1:10.015" to "[01:10.02]", or "9.014" to "[00:09.01]".
#[expect(clippy::cast_possible_truncation)]
fn parse_ttml_timestamp_as_lrc_timestamp(value: &str) -> Result<String> {
  let (mins, secs) = if let Some((m, s)) = value.split_once(':') {
    let mins = m.parse::<u32>()?;
    let secs = s.parse::<f64>()?;
    (mins, secs)
  } else {
    let secs = value.parse::<f64>()?;
    (0, secs)
  };

  let hundredths = (secs.fract() * 100.0).round() as u32;
  let secs = secs.floor() as u32;

  Ok(format!("[{mins:02}:{secs:02}.{hundredths:02}]"))
}

#[cfg(test)]
mod tests {
  use tracing::error;
  use tracing_test::traced_test;

  use super::*;

  #[test]
  #[traced_test]
  fn parse_ttml_into_lrc_lyrics() {
    let ttml_line_timing = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:lrc="http://lrc.red/lyric-ttml-internal" lrc:timing="Line" xml:lang="en"><head><metadata><sourceMetadata xmlns="http://lrc.red/lyric-ttml-internal" leadingSilence="0.180"><translations/><songwriters><songwriter>Daniel Bressanutti</songwriter><songwriter>Jean-Luc De Meyer</songwriter><songwriter>Patrick Codenys</songwriter><songwriter>Richard 23</songwriter></songwriters></sourceMetadata></metadata></head><body dur="00:04:57.330"><div begin="58.520" end="1:17.230"><p begin="58.520" end="1:00.390" lrc:key="L1">Today has no means</p><p begin="1:00.390" end="1:02.780" lrc:key="L2">He's alone and anonymous</p><p begin="1:02.780" end="1:04.310" lrc:key="L3">But written in his cells</p><p begin="1:04.310" end="1:09.660" lrc:key="L4">He's got the marks of a genius</p><p begin="1:09.660" end="1:11.940" lrc:key="L5">I'm looking for this man</p><p begin="1:11.940" end="1:17.230" lrc:key="L6">To sell him to other men</p></div><div begin="1:24.300" end="1:39.270"><p begin="1:24.300" end="1:28.200" lrc:key="L7">One, you lock the target</p><p begin="1:28.200" end="1:32.010" lrc:key="L8">Two, you bait the line</p><p begin="1:32.010" end="1:35.570" lrc:key="L9">Three, you slowly spread the net</p><p begin="1:35.570" end="1:39.270" lrc:key="L10">And four, you catch the man</p></div><div begin="1:44.350" end="1:46.930"><p begin="1:44.350" end="1:46.930" lrc:key="L11">You catch the man</p></div><div begin="1:53.530" end="2:02.240"><p begin="1:53.530" end="1:54.820" lrc:key="L12">I'm looking for this man</p><p begin="1:54.820" end="1:57.500" lrc:key="L13">To sell him to other men</p><p begin="1:57.500" end="1:59.030" lrc:key="L14">Sell him to other men</p><p begin="1:59.030" end="2:02.240" lrc:key="L15">At ten times his price, at least</p></div><div begin="2:04.490" end="2:25.560"><p begin="2:04.490" end="2:07.260" lrc:key="L16">Who's able to forget them</p><p begin="2:07.260" end="2:11.050" lrc:key="L17">To realize my aim</p><p begin="2:11.050" end="2:14.970" lrc:key="L18">One, you lock the target</p><p begin="2:14.970" end="2:17.830" lrc:key="L19">Two, you bait the line</p><p begin="2:17.830" end="2:20.670" lrc:key="L20">Bait the line, spread the net, catch the man</p><p begin="2:20.670" end="2:22.810" lrc:key="L21">Lock the target, bait the line</p><p begin="2:22.810" end="2:25.560" lrc:key="L22">Spread the net and catch the man</p></div><div begin="2:32.340" end="2:35.780"><p begin="2:32.340" end="2:35.780" lrc:key="L23">Freeze! Shoot to kill or die</p></div><div begin="2:51.940" end="2:57.000"><p begin="2:51.940" end="2:53.490" lrc:key="L24">I'm looking for this man</p><p begin="2:53.490" end="2:57.000" lrc:key="L25">To sell him to other men</p></div><div begin="3:07.730" end="3:31.740"><p begin="3:07.730" end="3:12.230" lrc:key="L26">Lock the target, bait the line</p><p begin="3:12.230" end="3:15.120" lrc:key="L27">Spread the net and catch the man</p><p begin="3:15.120" end="3:17.460" lrc:key="L28">Lock the target, bait the line</p><p begin="3:17.460" end="3:19.320" lrc:key="L29">Spread the net and catch the man</p><p begin="3:19.320" end="3:21.270" lrc:key="L30">Lock the target, bait the line</p><p begin="3:21.270" end="3:23.110" lrc:key="L31">Spread the net and catch the man</p><p begin="3:23.110" end="3:25.220" lrc:key="L32">Lock the target, bait the line</p><p begin="3:25.220" end="3:27.270" lrc:key="L33">Spread the net and catch the man</p><p begin="3:27.270" end="3:28.730" lrc:key="L34">Lock the target, bait the line</p><p begin="3:28.730" end="3:31.740" lrc:key="L35">Spread the net and catch the man</p></div><div begin="3:41.010" end="3:44.960"><p begin="3:41.010" end="3:44.960" lrc:key="L36">You catch the man</p></div><div begin="4:17.010" end="4:34.100"><p begin="4:17.010" end="4:20.660" lrc:key="L37">I'm looking for this man</p><p begin="4:20.660" end="4:27.430" lrc:key="L38">I'm looking for this man</p><p begin="4:27.430" end="4:29.530" lrc:key="L39">To sell him to other men</p><p begin="4:29.530" end="4:31.360" lrc:key="L40">Sell him to other men</p><p begin="4:31.360" end="4:34.100" lrc:key="L41">At ten times his price, at least</p></div><div begin="4:38.210" end="4:57.510"><p begin="4:38.210" end="4:41.590" lrc:key="L42">Freeze!</p><p begin="4:41.590" end="4:46.100" lrc:key="L43">Lock the target, spread the net and catch the man</p><p begin="4:46.100" end="4:50.120" lrc:key="L44">Spread the net, spread the net and catch the man</p><p begin="4:50.120" end="4:53.050" lrc:key="L45">Lock the target, spread the net and catch the man</p><p begin="4:53.050" end="4:54.800" lrc:key="L46">Lock the target, bait the line</p><p begin="4:54.800" end="4:57.510" lrc:key="L47">Spread the net and catch the man</p></div></body></tt>"#;

    let expected_line_timing = "[00:58.52]Today has no means\n[01:00.39]He's alone and anonymous\n[01:02.78]But written in his cells\n[01:04.31]He's got the marks of a genius\n[01:09.66]I'm looking for this man\n[01:11.94]To sell him to other men\n\n[01:24.30]One, you lock the target\n[01:28.20]Two, you bait the line\n[01:32.01]Three, you slowly spread the net\n[01:35.57]And four, you catch the man\n\n[01:44.35]You catch the man\n\n[01:53.53]I'm looking for this man\n[01:54.82]To sell him to other men\n[01:57.50]Sell him to other men\n[01:59.03]At ten times his price, at least\n\n[02:04.49]Who's able to forget them\n[02:07.26]To realize my aim\n[02:11.05]One, you lock the target\n[02:14.97]Two, you bait the line\n[02:17.83]Bait the line, spread the net, catch the man\n[02:20.67]Lock the target, bait the line\n[02:22.81]Spread the net and catch the man\n\n[02:32.34]Freeze! Shoot to kill or die\n\n[02:51.94]I'm looking for this man\n[02:53.49]To sell him to other men\n\n[03:07.73]Lock the target, bait the line\n[03:12.23]Spread the net and catch the man\n[03:15.12]Lock the target, bait the line\n[03:17.46]Spread the net and catch the man\n[03:19.32]Lock the target, bait the line\n[03:21.27]Spread the net and catch the man\n[03:23.11]Lock the target, bait the line\n[03:25.22]Spread the net and catch the man\n[03:27.27]Lock the target, bait the line\n[03:28.73]Spread the net and catch the man\n\n[03:41.01]You catch the man\n\n[04:17.01]I'm looking for this man\n[04:20.66]I'm looking for this man\n[04:27.43]To sell him to other men\n[04:29.53]Sell him to other men\n[04:31.36]At ten times his price, at least\n\n[04:38.21]Freeze!\n[04:41.59]Lock the target, spread the net and catch the man\n[04:46.10]Spread the net, spread the net and catch the man\n[04:50.12]Lock the target, spread the net and catch the man\n[04:53.05]Lock the target, bait the line\n[04:54.80]Spread the net and catch the man\n";

    let ttml_word_timing = r#"<tt lrc:byModel="true" xmlns="http://www.w3.org/ns/ttml" xmlns:lrc="http://lrc.red/lyric-ttml-internal" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" lrc:timing="Word" xml:lang="en"><head><metadata><ttm:agent type="person" xml:id="v1"/></metadata></head><body dur="3:31.162"><div begin="0:19.636" end="0:45.856"><p begin="0:19.636" end="0:24.316" lrc:key="L1" ttm:agent="v1"><span begin="0:19.636" end="0:20.007">Long</span> <span begin="0:20.007" end="0:20.535">afloat</span> <span begin="0:20.535" end="0:23.332">on</span> <span begin="0:23.332" end="0:23.695">ship</span><span begin="0:23.695" end="0:23.938">less</span> <span begin="0:23.938" end="0:24.316">oceans</span></p><p begin="0:26.655" end="0:31.256" lrc:key="L2" ttm:agent="v1"><span begin="0:26.655" end="0:27.090">I</span> <span begin="0:27.090" end="0:27.395">did</span> <span begin="0:27.395" end="0:28.956">all</span> <span begin="0:28.956" end="0:30.235">my</span> <span begin="0:30.235" end="0:30.575">best</span> <span begin="0:30.575" end="0:30.916">to</span> <span begin="0:30.916" end="0:31.256">smile</span></p><p begin="0:34.947" end="0:40.347" lrc:key="L3" ttm:agent="v1"><span begin="0:34.947" end="0:35.343">Till</span> <span begin="0:35.343" end="0:35.935">your</span> <span begin="0:35.935" end="0:36.456">singing</span> <span begin="0:36.456" end="0:38.067">eyes</span> <span begin="0:38.067" end="0:38.646">and</span> <span begin="0:38.646" end="0:39.136">fin</span><span begin="0:39.136" end="0:40.347">gers</span></p><p begin="0:41.675" end="0:45.856" lrc:key="L4" ttm:agent="v1"><span begin="0:41.675" end="0:42.143">Drew</span> <span begin="0:42.143" end="0:42.675">me</span> <span begin="0:42.675" end="0:43.175">lov</span><span begin="0:43.175" end="0:45.174">ing</span> <span begin="0:45.174" end="0:45.470">to</span> <span begin="0:45.470" end="0:45.836">your</span> <span begin="0:45.836" end="0:45.856">isle</span></p></div><div begin="0:49.935" end="0:50.535"><p begin="0:49.935" end="0:50.535" lrc:key="L5" ttm:agent="v1"><span begin="0:49.935" end="0:50.236">And</span> <span begin="0:50.236" end="0:50.505">you</span> <span begin="0:50.505" end="0:50.535">sang</span></p></div><div begin="0:53.976" end="1:01.176"><p begin="0:53.976" end="0:58.816" lrc:key="L6" ttm:agent="v1"><span begin="0:53.976" end="0:54.495">Sail</span> <span begin="0:54.495" end="0:54.722">to</span> <span begin="0:54.722" end="0:57.755">me,</span> <span begin="0:57.755" end="0:58.556">sail</span> <span begin="0:58.556" end="0:58.795">to</span> <span begin="0:58.795" end="0:58.816">me</span></p><p begin="0:59.776" end="1:01.176" lrc:key="L7" ttm:agent="v1"><span begin="0:59.776" end="1:00.016">Let</span> <span begin="1:00.016" end="1:00.255">me</span> <span begin="1:00.255" end="1:00.575">en</span><span begin="1:00.575" end="1:01.156">fold</span> <span begin="1:01.156" end="1:01.176">you</span></p></div><div begin="1:05.975" end="1:19.100"><p begin="1:05.975" end="1:10.275" lrc:key="L8" ttm:agent="v1"><span begin="1:05.975" end="1:06.455">Here</span> <span begin="1:06.455" end="1:06.755">I</span> <span begin="1:06.755" end="1:09.195">am,</span> <span begin="1:09.195" end="1:09.956">here</span> <span begin="1:09.956" end="1:10.255">I</span> <span begin="1:10.255" end="1:10.275">am</span></p><p begin="1:13.975" end="1:19.100" lrc:key="L9" ttm:agent="v1"><span begin="1:13.975" end="1:14.635">Wait</span><span begin="1:14.635" end="1:15.375">ing</span> <span begin="1:15.375" end="1:15.655">to</span> <span begin="1:15.655" end="1:19.080">hold</span> <span begin="1:19.080" end="1:19.100">you</span></p></div><div begin="1:24.226" end="1:50.495"><p begin="1:24.226" end="1:29.547" lrc:key="L10" ttm:agent="v1"><span begin="1:24.226" end="1:24.456">Did</span> <span begin="1:24.456" end="1:24.706">I</span> <span begin="1:24.706" end="1:27.135">dream</span> <span begin="1:27.135" end="1:27.535">you</span> <span begin="1:27.535" end="1:27.942">dreamed</span> <span begin="1:27.942" end="1:28.577">about</span> <span begin="1:28.577" end="1:29.547">me?</span></p><p begin="1:31.055" end="1:38.115" lrc:key="L11" ttm:agent="v1"><span begin="1:31.055" end="1:31.435">Were</span> <span begin="1:31.435" end="1:31.956">you</span> <span begin="1:31.956" end="1:34.415">here</span> <span begin="1:34.415" end="1:34.816">when</span> <span begin="1:34.816" end="1:35.626">I</span> <span begin="1:35.626" end="1:36.252">was</span> <span begin="1:36.252" end="1:37.295">full</span> <span begin="1:37.295" end="1:38.115">sail?</span></p><p begin="1:40.135" end="1:45.526" lrc:key="L12" ttm:agent="v1"><span begin="1:40.135" end="1:40.807">Now</span> <span begin="1:40.807" end="1:41.486">my</span> <span begin="1:41.486" end="1:41.555">fool</span><span begin="1:41.555" end="1:42.109">ish</span> <span begin="1:42.109" end="1:43.609">boat</span> <span begin="1:43.609" end="1:43.875">is</span> <span begin="1:43.875" end="1:44.348">lean</span><span begin="1:44.348" end="1:45.526">ing</span></p><p begin="1:46.774" end="1:50.495" lrc:key="L13" ttm:agent="v1"><span begin="1:46.774" end="1:46.855">Bro</span><span begin="1:46.855" end="1:47.428">ken</span> <span begin="1:47.428" end="1:49.415">lovelorn</span> <span begin="1:49.415" end="1:49.726">on</span> <span begin="1:49.726" end="1:50.316">your</span> <span begin="1:50.316" end="1:50.495">rocks</span></p></div><div begin="1:55.672" end="1:57.305"><p begin="1:55.672" end="1:57.305" lrc:key="L14" ttm:agent="v1"><span begin="1:55.672" end="1:55.956">For</span> <span begin="1:55.956" end="1:56.218">you</span> <span begin="1:56.218" end="1:57.305">sing</span></p></div><div begin="1:58.615" end="2:04.775"><p begin="1:58.615" end="2:02.661" lrc:key="L15" ttm:agent="v1"><span begin="1:58.615" end="1:59.355">Touch</span> <span begin="1:59.355" end="1:59.598">me</span> <span begin="1:59.598" end="2:01.735">not,</span> <span begin="2:01.735" end="2:02.415">touch</span> <span begin="2:02.415" end="2:02.641">me</span> <span begin="2:02.641" end="2:02.661">not</span></p><p begin="2:03.061" end="2:04.775" lrc:key="L16" ttm:agent="v1"><span begin="2:03.061" end="2:03.596">Come</span> <span begin="2:03.596" end="2:03.975">back</span> <span begin="2:03.975" end="2:04.217">to</span><span begin="2:04.217" end="2:04.755">mor</span><span begin="2:04.755" end="2:04.775">row</span></p></div><div begin="2:10.395" end="2:22.876"><p begin="2:10.395" end="2:14.555" lrc:key="L17" ttm:agent="v1"><span begin="2:10.395" end="2:11.146">Oh,</span> <span begin="2:11.146" end="2:11.496">my</span> <span begin="2:11.496" end="2:13.576">heart,</span> <span begin="2:13.576" end="2:14.036">oh,</span> <span begin="2:14.036" end="2:14.496">my</span> <span begin="2:14.496" end="2:14.555">heart</span></p><p begin="2:17.395" end="2:22.876" lrc:key="L18" ttm:agent="v1"><span begin="2:17.395" end="2:18.615">Shys</span> <span begin="2:18.615" end="2:18.956">from</span> <span begin="2:18.956" end="2:19.221">the</span> <span begin="2:19.221" end="2:22.856">sor</span><span begin="2:22.856" end="2:22.876">row</span></p></div><div begin="2:29.816" end="2:52.395"><p begin="2:29.816" end="2:34.276" lrc:key="L19" ttm:agent="v1"><span begin="2:29.816" end="2:29.976">Well,</span> <span begin="2:29.976" end="2:30.370">I'm</span> <span begin="2:30.370" end="2:30.636">as</span> <span begin="2:30.636" end="2:30.696">puz</span><span begin="2:30.696" end="2:32.196">zled</span> <span begin="2:32.196" end="2:32.649">as</span> <span begin="2:32.649" end="2:33.096">new</span><span begin="2:33.096" end="2:34.196">born</span> <span begin="2:34.196" end="2:34.276">child</span></p><p begin="2:36.636" end="2:39.316" lrc:key="L20" ttm:agent="v1"><span begin="2:36.636" end="2:36.976">I'm</span> <span begin="2:36.976" end="2:37.354">as</span> <span begin="2:37.354" end="2:37.436">rid</span><span begin="2:37.436" end="2:38.696">dled</span> <span begin="2:38.696" end="2:38.976">as</span> <span begin="2:38.976" end="2:39.282">the</span> <span begin="2:39.282" end="2:39.316">tide</span></p><p begin="2:43.036" end="2:47.515" lrc:key="L21" ttm:agent="v1"><span begin="2:43.036" end="2:43.335">Should</span> <span begin="2:43.335" end="2:43.615">I</span> <span begin="2:43.615" end="2:45.256">stand</span> <span begin="2:45.256" end="2:45.536">amid</span> <span begin="2:45.536" end="2:45.736">the</span> <span begin="2:45.736" end="2:46.136">break</span><span begin="2:46.136" end="2:47.515">ers?</span></p><p begin="2:48.656" end="2:52.395" lrc:key="L22" ttm:agent="v1"><span begin="2:48.656" end="2:48.970">Or</span> <span begin="2:48.970" end="2:49.196">should</span> <span begin="2:49.196" end="2:49.448">I</span> <span begin="2:49.448" end="2:51.011">lie</span> <span begin="2:51.011" end="2:51.476">with</span> <span begin="2:51.476" end="2:52.053">death,</span> <span begin="2:52.053" end="2:52.356">my</span> <span begin="2:52.356" end="2:52.395">bride?</span></p></div><div begin="2:55.776" end="2:56.436"><p begin="2:55.776" end="2:56.436" lrc:key="L23" ttm:agent="v1"><span begin="2:55.776" end="2:56.115">Hear</span> <span begin="2:56.115" end="2:56.395">me</span> <span begin="2:56.395" end="2:56.436">sing</span></p></div><div begin="2:58.256" end="3:05.670"><p begin="2:58.256" end="3:02.210" lrc:key="L24" ttm:agent="v1"><span begin="2:58.256" end="2:58.736">Swim</span> <span begin="2:58.736" end="2:58.953">to</span> <span begin="2:58.953" end="3:01.056">me,</span> <span begin="3:01.056" end="3:01.956">swim</span> <span begin="3:01.956" end="3:02.190">to</span> <span begin="3:02.190" end="3:02.210">me</span></p><p begin="3:02.916" end="3:05.670" lrc:key="L25" ttm:agent="v1"><span begin="3:02.916" end="3:03.196">Let</span> <span begin="3:03.196" end="3:03.236">me</span> <span begin="3:03.236" end="3:03.727">en</span><span begin="3:03.727" end="3:04.356">fold</span> <span begin="3:04.356" end="3:05.670">you</span></p></div><div begin="3:08.056" end="3:21.856"><p begin="3:08.056" end="3:12.596" lrc:key="L26" ttm:agent="v1"><span begin="3:08.056" end="3:08.835">Here</span> <span begin="3:08.835" end="3:09.076">I</span> <span begin="3:09.076" end="3:11.136">am,</span> <span begin="3:11.136" end="3:12.336">here</span> <span begin="3:12.336" end="3:12.576">I</span> <span begin="3:12.576" end="3:12.596">am</span></p><p begin="3:15.816" end="3:21.856" lrc:key="L27" ttm:agent="v1"><span begin="3:15.816" end="3:16.336">Wait</span><span begin="3:16.336" end="3:16.796">ing</span> <span begin="3:16.796" end="3:17.086">to</span> <span begin="3:17.086" end="3:21.836">hold</span> <span begin="3:21.836" end="3:21.856">you</span></p></div></body></tt>"#;

    let expected_word_timing = "[00:19.64]Long afloat on shipless oceans\n[00:26.66]I did all my best to smile\n[00:34.95]Till your singing eyes and fingers\n[00:41.67]Drew me loving to your isle\n\n[00:49.94]And you sang\n\n[00:53.98]Sail to me, sail to me\n[00:59.78]Let me enfold you\n\n[01:05.97]Here I am, here I am\n[01:13.97]Waiting to hold you\n\n[01:24.23]Did I dream you dreamed about me?\n[01:31.05]Were you here when I was full sail?\n[01:40.13]Now my foolish boat is leaning\n[01:46.77]Broken lovelorn on your rocks\n\n[01:55.67]For you sing\n\n[01:58.62]Touch me not, touch me not\n[02:03.06]Come back tomorrow\n\n[02:10.39]Oh, my heart, oh, my heart\n[02:17.39]Shys from the sorrow\n\n[02:29.82]Well, I'm as puzzled as newborn child\n[02:36.64]I'm as riddled as the tide\n[02:43.04]Should I stand amid the breakers?\n[02:48.66]Or should I lie with death, my bride?\n\n[02:55.78]Hear me sing\n\n[02:58.26]Swim to me, swim to me\n[03:02.92]Let me enfold you\n\n[03:08.06]Here I am, here I am\n[03:15.82]Waiting to hold you\n";

    let ttml_no_timing = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:lrc="http://lrc.red/lyric-ttml-internal" lrc:timing="None" xml:lang="en"><head><metadata><sourceMetadata xmlns="http://lrc.red/lyric-ttml-internal" leadingSilence="0.060"><translations/><songwriters><songwriter>Jeremy Fromm Malvin</songwriter><songwriter>Laurel Mae Arnell-Cullen</songwriter></songwriters></sourceMetadata></metadata></head><body><div><p>Gentle in a dream</p><p>Creeping up on me</p><p>When its four or five in the morning</p><p>Turn the tv off</p><p>Stand out on the lawn</p><p>and shout your name as if you might call mine too</p><p>On a Saturday night</p><p>shouldn’t be thinking about you but I’m wondering why</p><p>I found your blood on the sand</p><p>I go to call you but can’t</p><p>CHORUS</p><p>Sometimes I feel your body rush past me at night (ah)</p><p>Sometimes I see your eyes when a stranger looks a mine</p><p>Taste the morning blues</p><p>hear the birdsong</p><p>Sounds a bit like you</p><p>All through the afternoon</p><p>A red sun burns hot</p><p>Feels a lot like you</p><p>Freeze on a stage</p><p>birthed a heavy rage</p><p>now I’m hungry for emotion</p><p>Lay in the bath</p><p>close my eyes to a sadness</p><p>I’m deep under an ocean blue</p><p>Caught in a fight</p><p>chased you under the heat</p><p>my tempers rose through the ceiling</p><p>Found your blood on the snow</p><p>when I was tryna let go</p><p>Chorus</p><p>You my love, you oh my love</p><p>You my love, you oh my love</p><p>Wind in my hair I feel your palms</p><p>When you’re in the air it makes me calm</p><p>Cradle me throughout the storm</p><p>Always wake up at the dawn</p><p>On the Lawn, when you call</p><p>Catch me undone</p></div></body></tt>"#;

    let expected_no_timing = "Gentle in a dream\nCreeping up on me\nWhen its four or five in the morning\nTurn the tv off\nStand out on the lawn\nand shout your name as if you might call mine too\nOn a Saturday night\nshouldn’t be thinking about you but I’m wondering why\nI found your blood on the sand\nI go to call you but can’t\nCHORUS\nSometimes I feel your body rush past me at night (ah)\nSometimes I see your eyes when a stranger looks a mine\nTaste the morning blues\nhear the birdsong\nSounds a bit like you\nAll through the afternoon\nA red sun burns hot\nFeels a lot like you\nFreeze on a stage\nbirthed a heavy rage\nnow I’m hungry for emotion\nLay in the bath\nclose my eyes to a sadness\nI’m deep under an ocean blue\nCaught in a fight\nchased you under the heat\nmy tempers rose through the ceiling\nFound your blood on the snow\nwhen I was tryna let go\nChorus\nYou my love, you oh my love\nYou my love, you oh my love\nWind in my hair I feel your palms\nWhen you’re in the air it makes me calm\nCradle me throughout the storm\nAlways wake up at the dawn\nOn the Lawn, when you call\nCatch me undone\n";

    let res_line_timing = parse_ttml_as_lrc(ttml_line_timing)
      .inspect_err(|e| error!("{e}"))
      .map_or_default(|l| l.contents);
    assert_eq!(res_line_timing, expected_line_timing, "TTML with line-level timestamps");

    let res_word_timing = parse_ttml_as_lrc(ttml_word_timing)
      .inspect_err(|e| error!("{e}"))
      .map_or_default(|l| l.contents);
    assert_eq!(res_word_timing, expected_word_timing, "TTML with word-level timestamps");

    let res_no_timing = parse_ttml_as_lrc(ttml_no_timing)
      .inspect_err(|e| error!("{e}"))
      .map_or_default(|l| l.contents);
    assert_eq!(res_no_timing, expected_no_timing, "TTML with no timestamps");
  }
}
