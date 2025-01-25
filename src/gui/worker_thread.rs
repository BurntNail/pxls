use crate::logic::{
    dither_palette, get_palette, DistanceAlgorithm, OutputSettings, PaletteSettings,
};
use image::{DynamicImage, ImageReader, Rgba};
use rfd::FileDialog;
use std::env::current_dir;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

pub enum ThreadRequest {
    GetFile,
    RenderPalette {
        input: DynamicImage,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
    },
    RenderOutput {
        input: DynamicImage,
        palette: Vec<Rgba<u8>>,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
    },
}

pub enum ThreadResult {
    ReadInFile(DynamicImage),
    RenderedPalette(DynamicImage, Vec<Rgba<u8>>),
    RenderedImage {
        input: DynamicImage,
        palette: Vec<Rgba<u8>>,
        output: DynamicImage,
    },
}

#[allow(clippy::type_complexity)]
pub fn start_worker_thread() -> (
    JoinHandle<()>,
    Sender<ThreadRequest>,
    Receiver<ThreadResult>,
    Receiver<(u32, u32)>,
    Arc<AtomicBool>,
) {
    let (req_tx, req_rx) = channel();
    let (res_tx, res_rx) = channel();
    let (prog_tx, prog_rx) = channel();
    let should_stop = Arc::new(AtomicBool::new(false));
    let ret_should_stop = should_stop.clone();

    let handle = std::thread::spawn(move || loop {
        if should_stop.load(Ordering::Relaxed) {
            break;
        }

        for req in req_rx.try_iter() {
            match req {
                ThreadRequest::GetFile => {
                    if let Some(file) = FileDialog::new()
                        .add_filter("Images", &["jpg", "png", "jpeg"])
                        .set_directory(current_dir().unwrap_or_else(|_| "/".into()))
                        .pick_file()
                    {
                        match ImageReader::open(file) {
                            Ok(img) => match img.decode() {
                                Ok(img) => {
                                    res_tx.send(ThreadResult::ReadInFile(img)).unwrap();
                                }
                                Err(e) => {
                                    eprintln!("Error decoding image: {e:?}");
                                }
                            },
                            Err(e) => {
                                eprintln!("Error reading image file: {e:?}");
                            }
                        }
                    }
                }
                ThreadRequest::RenderPalette {
                    input,
                    palette_settings,
                    distance_algorithm,
                } => {
                    let palette =
                        get_palette(&input, palette_settings, distance_algorithm, &prog_tx);

                    res_tx
                        .send(ThreadResult::RenderedPalette(input, palette))
                        .unwrap();
                }
                ThreadRequest::RenderOutput {
                    input,
                    palette,
                    output_settings,
                    distance_algorithm,
                } => {
                    let output = dither_palette(
                        &input,
                        &palette,
                        distance_algorithm,
                        output_settings,
                        &prog_tx,
                    );

                    res_tx
                        .send(ThreadResult::RenderedImage {
                            input,
                            palette,
                            output,
                        })
                        .unwrap();
                }
            }
        }
    });

    (handle, req_tx, res_rx, prog_rx, ret_should_stop)
}
