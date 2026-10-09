use media_rip_core::{EngineEvent, MediaEngine, ScanOptions, ports::CancellationToken};
fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        media_rip_engine::OpenMediaEngine::new().scan(
            "/dev/sr0".into(),
            ScanOptions {
                allow_online_access: false,
                ..Default::default()
            },
            tx,
            CancellationToken::default(),
        );
    });
    for event in rx {
        match event {
            EngineEvent::ScanFinished(catalog) => {
                std::fs::write(
                    "/tmp/matrix-catalog.json",
                    serde_json::to_vec_pretty(&catalog).unwrap(),
                )
                .unwrap();
                println!("Saved /tmp/matrix-catalog.json");
            }
            event => eprintln!("{event:?}"),
        }
    }
}
