fn main() -> eframe::Result {
    eframe::run_native(
        "Player",
        eframe::NativeOptions::default(),
        Box::new(|cc| Ok(Box::new(player::TrackPlayer::new(cc)))),
    )
}
