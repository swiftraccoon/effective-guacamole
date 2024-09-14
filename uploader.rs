use dotenv::dotenv;
use notify::{recommended_watcher, RecursiveMode, Result as NotifyResult, Watcher};
use regex::Regex;
use reqwest::{Client, multipart::{Form, Part}};
use std::{env, path::PathBuf, sync::mpsc::channel};
use tokio::runtime::Runtime;
use std::fs;

fn main() -> NotifyResult<()> {
    dotenv().ok();
    let monitored_directory = env::var("MONITORED_DIRECTORY")
        .expect("MONITORED_DIRECTORY environment variable not set");
    let root_path_buf = PathBuf::from(&monitored_directory);
    println!("Monitoring directory: {:?}", root_path_buf);

    let rt = Runtime::new().unwrap();
    // Changing the block to handle Result
    rt.block_on(async {
        let (tx, rx) = channel();

        // Handle result of recommended_watcher
        let mut watcher = recommended_watcher(move |res| tx.send(res).unwrap()).unwrap();
        // Handle result of watcher.watch
        watcher.watch(&root_path_buf, RecursiveMode::Recursive).unwrap();

        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to create HTTP client");
        while let Ok(event) = rx.recv() {
            match event {
                Ok(event) => {
                    println!("Processing event: {:?}", event);
                    for path in event.paths {
                        println!("Detected change in path: {:?}", path);
                        if should_process_file(&path, &root_path_buf) {
                            if let Some((mp3_path, txt_path)) = extract_file_info(&path) {
                                upload_file(&client, &mp3_path, &txt_path).await;
                            }
                        }
                    }
                },
                Err(e) => eprintln!("Error handling event: {:?}", e),
            }
        }
    });

    Ok(())
}

fn should_process_file(file_path: &PathBuf, root_path: &PathBuf) -> bool {
    let should_process = file_path.parent() != Some(root_path) && file_path.is_file();
    println!("Should process {:?}: {}", file_path, should_process);
    should_process
}

async fn upload_file(client: &Client, mp3_path: &PathBuf, txt_path: &PathBuf) {
    println!("Uploading files: {:?}, {:?}", mp3_path, txt_path);
    let filename = mp3_path.file_name().unwrap().to_str().unwrap();
    if let Some((timestamp, talkgroup_id, radio_id)) = parse_filename(filename) {
        let mp3_bytes = fs::read(mp3_path).expect("Failed to read mp3 file");
        let txt_bytes = fs::read(txt_path).expect("Failed to read txt file");
        println!("timestamp: {:?} \n talkgroup_id: {:?} \n radio_id: {:?}", timestamp, talkgroup_id, radio_id);
        let mp3_part = Part::bytes(mp3_bytes).file_name(filename.to_string()).mime_str("audio/mpeg").expect("Invalid MIME type");
        let txt_filename = txt_path.file_name().unwrap().to_str().unwrap();
        let txt_part = Part::bytes(txt_bytes).file_name(txt_filename.to_string()).mime_str("text/plain").expect("Invalid MIME type");

        let form = Form::new()
            .text("talkgroupId", talkgroup_id)
            .text("timestamp", timestamp)
            .text("radioId", radio_id)
            .part("mp3", mp3_part)
            .part("transcription", txt_part);

        match client.post("https://some.host:3000/api/upload")
            .header("X-API-Key", "12345678")
            .multipart(form)
            .send()
            .await {
                Ok(response) => println!("Upload successful: {:?}", response),
                Err(e) => eprintln!("Upload failed: {}", e),
            }
    }
}

fn extract_file_info(file_path: &PathBuf) -> Option<(PathBuf, PathBuf)> {
    println!("Extracting file info for: {:?}", file_path);
    let file_stem = file_path.file_stem()?.to_str()?;
    let parent_dir = file_path.parent()?;
    let mp3_path = parent_dir.join(format!("{}.mp3", file_stem));
    let txt_path = parent_dir.join(format!("{}.txt", file_stem));

    if mp3_path.exists() && txt_path.exists() {
        Some((mp3_path, txt_path))
    } else {
        println!("Either MP3 or TXT file does not exist");
        None
    }
}

fn parse_filename(filename: &str) -> Option<(String, String, String)> {
    println!("Parsing filename: {}", filename);
    // This regex is designed to match the timestamp, talkgroup ID, and optionally the radio ID.
    // It defaults to "123456" if the radio ID is not found.
    let re = Regex::new(
        r"(\d{8}_\d{6}).*__TO_(\d+)(?:_FROM_(\d+))?"
    ).unwrap();

    re.captures(filename).and_then(|cap| {
        let timestamp = cap.get(1)?.as_str().to_string();
        let talkgroup_id = cap.get(2)?.as_str().to_string();
        // Use the captured radio ID if present; otherwise, default to "123456".
        let radio_id = cap.get(3).map_or("123456".to_string(), |m| m.as_str().to_string());
        Some((timestamp, talkgroup_id, radio_id))
    })
}
