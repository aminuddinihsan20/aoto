//! "Play on": where the phone's sound goes. Android decides that (its speaker, a
//! Bluetooth or wired device) and holds the media volume; the activity
//! (`AudioOutput.java`) reports both here when they change, and the app asks it for the
//! system's output switcher and for a volume. Nothing here is polled.

use std::sync::Mutex;

const FIELD: char = '\u{1f}';

/// What kind of device the music plays through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The phone's own speaker.
    Phone,
    Bluetooth,
    /// Headphones or a line on the jack.
    Wired,
    Usb,
    /// A screen or a dock: HDMI and the like.
    External,
    HearingAid,
    Other,
}

impl Kind {
    /// The number the activity (Java) knows it by.
    pub fn code(self) -> i32 {
        match self {
            Kind::Phone => 0,
            Kind::Bluetooth => 1,
            Kind::Wired => 2,
            Kind::Usb => 3,
            Kind::External => 4,
            Kind::HearingAid => 5,
            Kind::Other => 6,
        }
    }

    pub fn from_code(code: i32) -> Option<Kind> {
        Some(match code {
            0 => Kind::Phone,
            1 => Kind::Bluetooth,
            2 => Kind::Wired,
            3 => Kind::Usb,
            4 => Kind::External,
            5 => Kind::HearingAid,
            6 => Kind::Other,
            _ => return None,
        })
    }
}

/// What the app can open for the listener to pick another output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switcher {
    None,
    /// Android's own output switcher, over the app.
    System,
    /// Only the phone's Bluetooth settings (Android 9 and older).
    BluetoothSettings,
}

/// Where the music plays, and the phone's media volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub kind: Kind,
    /// The device's own name ("Pixel Buds Pro"); empty when it gives none.
    pub name: String,
    /// The media volume, in the phone's steps from 0 to `max_volume`.
    pub volume: u32,
    /// 0: the volume can't be set here (a device with a fixed volume).
    pub max_volume: u32,
    pub switcher: Switcher,
}

impl Default for Output {
    /// Until the activity reports (and on a desktop, where nothing does).
    fn default() -> Self {
        Output { kind: Kind::Phone, name: String::new(), volume: 0, max_volume: 0, switcher: Switcher::None }
    }
}

impl Output {
    /// The activity's report: kind, name, volume, its most and the switcher, joined by
    /// U+001F. `None` for a line that isn't one.
    pub fn from_line(line: &str) -> Option<Output> {
        let mut f = line.split(FIELD);
        let (kind, name, volume, max, switcher) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
        let max_volume: u32 = max.parse().ok()?;
        let switcher = match switcher {
            "0" => Switcher::None,
            "1" => Switcher::System,
            "2" => Switcher::BluetoothSettings,
            _ => return None,
        };
        Some(Output { kind: Kind::from_code(kind.parse().ok()?)?, name: name.to_owned(), volume: volume.parse::<u32>().ok()?.min(max_volume), max_volume, switcher })
    }

    /// What the output is called: its own name, or what kind of thing it is.
    pub fn title(&self) -> &str {
        match self.kind {
            Kind::Phone => "This phone",
            _ if !self.name.is_empty() => &self.name,
            Kind::Bluetooth => "Bluetooth device",
            Kind::Wired => "Wired headphones",
            Kind::Usb => "USB audio",
            Kind::External => "Screen or dock",
            Kind::HearingAid => "Hearing aid",
            Kind::Other => "Another device",
        }
    }

    /// How the sound gets there, as the line under the title.
    pub fn detail(&self) -> &'static str {
        match self.kind {
            Kind::Phone => "Phone speaker",
            Kind::Bluetooth => "Bluetooth",
            Kind::Wired => "Wired",
            Kind::Usb => "USB",
            Kind::External => "HDMI or dock",
            Kind::HearingAid => "Hearing aid",
            Kind::Other => "Audio device",
        }
    }
}

static OUTPUT: Mutex<Option<Output>> = Mutex::new(None);

/// The activity: the output or the media volume changed. A line that can't be read
/// changes nothing.
pub fn set_output_line(line: &str) {
    if let Some(output) = Output::from_line(line) {
        *OUTPUT.lock().unwrap_or_else(|e| e.into_inner()) = Some(output);
        crate::platform::wake_ui();
    }
}

/// Where the music plays now.
pub fn output() -> Output {
    OUTPUT.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// Ask the activity for the output switcher (or the Bluetooth settings, where Android
/// has none).
pub fn open_switcher() {
    crate::platform::post("output-switch", "");
}

/// Ask the activity to set the media volume to `index` of the phone's steps.
pub fn set_volume(index: u32) {
    crate::platform::post("output-volume", &index.to_string());
}

/// How long the slider keeps to what the listener set while the phone hasn't followed.
const HOLD_SECS: f64 = 1.0;

/// The volume slider's own memory. The phone follows a moment after it is asked, and
/// its report of the old volume must not pull the slider back meanwhile.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slider {
    /// The step last asked for, and when.
    asked: Option<(u32, f64)>,
}

impl Slider {
    /// The listener put the slider on `step`: true when the phone is to be asked for it
    /// (not again for the step it was just asked for).
    pub fn set(&mut self, step: u32, now: f64) -> bool {
        let new = self.asked.is_none_or(|(asked, _)| asked != step);
        self.asked = Some((step, now));
        new
    }

    /// Is the phone still to follow what was asked of it?
    pub fn waiting(&self) -> bool {
        self.asked.is_some()
    }

    /// The step to show: what was asked for until the phone reports it (or a second
    /// went by without that), then whatever the phone says, as when its keys change it.
    pub fn shown(&mut self, phone: u32, now: f64) -> u32 {
        match self.asked {
            Some((asked, at)) if asked != phone && now - at < HOLD_SECS => asked,
            _ => {
                self.asked = None;
                phone
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(fields: &[&str]) -> String {
        fields.join("\u{1f}")
    }

    #[test]
    fn the_activitys_report_is_read_field_by_field() {
        let o = Output::from_line(&line(&["1", "Pixel Buds Pro", "7", "15", "1"])).unwrap();
        assert_eq!(o, Output { kind: Kind::Bluetooth, name: "Pixel Buds Pro".into(), volume: 7, max_volume: 15, switcher: Switcher::System });
        let o = Output::from_line(&line(&["0", "", "25", "25", "2"])).unwrap();
        assert_eq!((o.kind, o.switcher, o.volume), (Kind::Phone, Switcher::BluetoothSettings, 25));
    }

    #[test]
    fn a_report_that_is_not_one_is_refused() {
        assert_eq!(Output::from_line(""), None);
        assert_eq!(Output::from_line(&line(&["1", "Buds", "7", "15"])), None, "a field short");
        assert_eq!(Output::from_line(&line(&["9", "Buds", "7", "15", "1"])), None, "an unknown kind");
        assert_eq!(Output::from_line(&line(&["1", "Buds", "loud", "15", "1"])), None);
        assert_eq!(Output::from_line(&line(&["1", "Buds", "7", "15", "5"])), None, "an unknown switcher");
    }

    #[test]
    fn a_volume_past_its_most_is_held_to_it() {
        let o = Output::from_line(&line(&["0", "", "40", "25", "0"])).unwrap();
        assert_eq!(o.volume, 25);
    }

    #[test]
    fn every_kind_has_a_code_for_the_activity_and_back() {
        for kind in [Kind::Phone, Kind::Bluetooth, Kind::Wired, Kind::Usb, Kind::External, Kind::HearingAid, Kind::Other] {
            assert_eq!(Kind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(Kind::from_code(7), None);
        assert_eq!(Kind::Phone.code(), 0, "AudioOutput.java counts them from the phone");
    }

    #[test]
    fn the_phones_speaker_is_this_phone_whatever_its_model_is_called() {
        let o = Output { name: "Pixel 8".into(), ..Output::default() };
        assert_eq!(o.title(), "This phone");
        assert_eq!(o.detail(), "Phone speaker");
    }

    #[test]
    fn a_device_goes_by_its_own_name() {
        let o = Output { kind: Kind::Bluetooth, name: "Pixel Buds Pro".into(), ..Output::default() };
        assert_eq!(o.title(), "Pixel Buds Pro");
        assert_eq!(o.detail(), "Bluetooth");
    }

    #[test]
    fn a_device_without_a_name_is_called_by_its_kind() {
        let named = |kind| Output { kind, ..Output::default() }.title().to_owned();
        assert_eq!(named(Kind::Bluetooth), "Bluetooth device");
        assert_eq!(named(Kind::Wired), "Wired headphones");
        assert_eq!(named(Kind::Usb), "USB audio");
        assert_eq!(named(Kind::External), "Screen or dock");
        assert_eq!(named(Kind::HearingAid), "Hearing aid");
        assert_eq!(named(Kind::Other), "Another device");
    }

    #[test]
    fn nothing_reported_is_this_phone_with_no_volume_to_set_and_nothing_to_open() {
        let o = Output::default();
        assert_eq!((o.title(), o.max_volume, o.switcher), ("This phone", 0, Switcher::None));
    }

    #[test]
    fn the_newest_report_is_what_the_app_sees() {
        set_output_line(&line(&["2", "", "3", "15", "1"]));
        assert_eq!(output().kind, Kind::Wired);
        set_output_line("nonsense");
        assert_eq!(output().kind, Kind::Wired, "a broken report changes nothing");
        set_output_line(&line(&["0", "", "3", "15", "1"]));
        assert_eq!(output().kind, Kind::Phone);
    }

    #[test]
    fn the_phone_is_asked_once_for_each_step_the_slider_reaches() {
        let mut s = Slider::default();
        assert!(s.set(7, 0.0));
        assert!(!s.set(7, 0.1), "the same step again as the finger rests");
        assert!(s.set(8, 0.2));
        assert!(s.set(7, 0.3), "back to an earlier step is a new ask");
    }

    #[test]
    fn the_slider_keeps_to_what_was_set_until_the_phone_follows() {
        let mut s = Slider::default();
        s.set(9, 10.0);
        assert_eq!(s.shown(5, 10.1), 9, "the phone still says the old volume");
        assert_eq!(s.shown(9, 10.3), 9);
        assert_eq!(s.shown(11, 10.4), 11, "then the phone's keys move it");
    }

    #[test]
    fn a_phone_that_never_follows_wins_after_a_second() {
        let mut s = Slider::default();
        s.set(9, 10.0);
        assert_eq!(s.shown(5, 10.9), 9);
        assert_eq!(s.shown(5, 11.1), 5, "the volume couldn't be set: show what it is");
        assert!(s.set(9, 11.2), "and asking again is a new ask");
    }

    #[test]
    fn with_nothing_set_the_slider_shows_the_phones_volume() {
        let mut s = Slider::default();
        assert_eq!(s.shown(4, 0.0), 4);
        assert_eq!(s.shown(6, 5.0), 6);
    }
}
