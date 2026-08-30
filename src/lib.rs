use eframe::egui;
use eframe::egui::{RichText, Ui};

#[cfg(target_os = "android")]
mod android;

// A music player for real audio files.
//   - Android: orchestrates the SAF file picker (Java shim) and plays the bytes
//     the user selects. See `src/java/.../PlayerActivity.java` + `android.rs`.
//   - Desktop: falls back to a synthesized track so the app still demos.
//
// :ponytail: Whole files go into memory; no playlist, no seeking, no metadata
// (artist/album/artwork). CEILING: grow toward streaming + a proper song list.
// UPGRADE: MediaStore scan for a library view; predictive device audio focus.

pub struct TrackPlayer {
    // Keep the audio output device alive for the app's lifetime.
    sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    playing: bool,
    title: String,
}

impl TrackPlayer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Self {
            sink: None,
            player: None,
            playing: false,
            title: String::from("Pick a song to play it"),
        }
    }

    /// Main button: pause / resume a loaded track, otherwise pick a song
    /// (or play the synth fallback on desktop).
    fn primary(&mut self) {
        if self.playing {
            if let Some(p) = &self.player {
                p.pause();
            }
            self.playing = false;
        } else if self.has_track() {
            if let Some(p) = &self.player {
                p.play();
            }
            self.playing = true;
        } else {
            #[cfg(target_os = "android")]
            crate::android::pick_audio();
            #[cfg(not(target_os = "android"))]
            self.play_synth();
        }
    }

    fn has_track(&self) -> bool {
        self.player.as_ref().is_some_and(|p| !p.empty())
    }

    fn play_bytes(&mut self, bytes: Vec<u8>, name: String) {
        if self.sink.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => self.sink = Some(sink),
                Err(_) => {
                    // No audio device: keep the UI responsive but don't crash.
                    self.title = "No audio device".to_string();
                    return;
                }
            }
        }
        if self.player.is_none() {
            let mixer = self.sink.as_ref().unwrap().mixer();
            self.player = Some(rodio::Player::connect_new(mixer));
        }
        let Some(p) = &self.player else { return };
        p.stop();
        match rodio::Decoder::new(std::io::Cursor::new(bytes)) {
            Ok(src) => {
                p.append(src);
                p.play();
                self.playing = true;
                self.title = name;
            }
            Err(_) => self.title = format!("Unsupported audio format: {name}"),
        }
    }

    #[cfg(not(target_os = "android"))]
    fn play_synth(&mut self) {
        self.play_bytes(synth_track(), String::from("C Major Arpeggio (generated)"));
    }

    #[cfg(target_os = "android")]
    fn android_step(&mut self) {
        while let Some(msg) = crate::android::poll() {
            match msg {
                crate::android::Msg::Picked { uri } => {
                    let name = uri
                        .rsplit('/')
                        .next()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("song")
                        .to_string();
                    self.title = format!("Loading {name}…");
                    crate::android::load_async(uri, name);
                }
                crate::android::Msg::Loaded { bytes, name } => {
                    self.play_bytes(bytes, name);
                }
                crate::android::Msg::Error(e) => self.title = e,
            }
        }
    }
}

impl eframe::App for TrackPlayer {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "android")]
        self.android_step();

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new(&self.title).size(22.0).strong());
                ui.add_space(30.0);

                let label = if self.playing {
                    "⏸ Pause"
                } else if self.has_track() {
                    "▶ Play"
                } else {
                    "🎵 Pick a Song"
                };
                if ui
                    .add_sized([160.0, 56.0], egui::Button::new(RichText::new(label).size(20.0)))
                    .clicked()
                {
                    self.primary();
                }
            });
        });
    }
}

#[cfg(not(target_os = "android"))]
fn synth_track() -> Vec<u8> {
    // Generate a short PCM track: 2 bars of a C major arpeggio at 44.1kHz mono.
    let sample_rate = 44_100u32;
    let seconds = 4u32;
    let n_samples = (sample_rate * seconds) as usize;
    let mut pcm = Vec::with_capacity(n_samples * 2);

    let notes: [f32; 8] = [261.63, 329.63, 392.0, 523.25, 392.0, 329.63, 261.63, 196.0];
    let note_len = n_samples / notes.len();

    for i in 0..n_samples {
        let note = notes[i / note_len];
        let t = i as f32 / sample_rate as f32;
        let v = (std::f32::consts::TAU * note * t).sin() * 0.3;
        let v = (v * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }

    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

// --- Android entry point (loaded by NativeActivity via cargo-apk2) ---
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("player"),
    );
    let options = eframe::NativeOptions {
        android_app: Some(app),
        ..Default::default()
    };
    eframe::run_native(
        "Player",
        options,
        Box::new(|cc| Ok(Box::new(TrackPlayer::new(cc)))),
    )
    .expect("run native");
}