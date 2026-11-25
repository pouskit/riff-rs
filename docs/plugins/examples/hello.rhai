// @name Hello
// @author Riff
// @version 1.0.0
// @description Shows a notification when a new track starts.
// @permissions notify

// Called when the track changes. `track` is a map, or () when nothing plays.
fn on_trackchange(track) {
  if track != () && track.title != () {
    notify("Now playing: " + track.title);
  }
}
