use eframe::egui;
use eframe::egui::{RichText, Ui};

// A minimal music player: plays a self-contained synthesized track through rodio.
//
// :ponytail: No real on-device file picking — we synthesize audio in-memory so the
// app is trivially testable everywhere with zero Android-native plumbing.
// CEILING: cannot play the user's own music library. UPGRADE: read files via the
// Android Storage Access Framework (needs `jni` + a small Java intent) or MediaStore,
// and feed the returned bytes to rodio::Decoder::new(BufReader).

pub struct TrackPlayer {
    // Keep the audio output device alive for the app's lifetime.
    sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    playing: bool,
    title: String,
}

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

impl TrackPlayer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Self {
            sink: None,
            player: None,
            playing: false,
            title: String::from("C Major Arpeggio (generated)"),
        }
    }

    fn toggle(&mut self) {
        if self.playing {
            if let Some(p) = &self.player {
                p.pause();
            }
            self.playing = false;
        } else {
            // Lazily open the audio device on first play so headless runs don't crash.
            if self.sink.is_none() {
                match rodio::DeviceSinkBuilder::open_default_sink() {
                    Ok(sink) => self.sink = Some(sink),
                    Err(_) => {
                        // No audio device: keep the UI responsive but don't crash.
                        self.playing = true;
                        return;
                    }
                }
            }
            if self.player.is_none() {
                let mixer = self.sink.as_ref().unwrap().mixer();
                let player = rodio::Player::connect_new(mixer);
                self.player = Some(player);
            }
            let player = self.player.as_ref().unwrap();
            if player.is_paused() || player.empty() {
                if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(synth_track())) {
                    player.append(src);
                }
            }
            player.play();
            self.playing = true;
        }
    }
}

impl eframe::App for TrackPlayer {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new(&self.title).size(22.0).strong());
                ui.add_space(30.0);

                let label = if self.playing { "⏸ Pause" } else { "▶ Play" };
                if ui
                    .add_sized([160.0, 56.0], egui::Button::new(RichText::new(label).size(20.0)))
                    .clicked()
                {
                    self.toggle();
                }
            });
        });
    }
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
