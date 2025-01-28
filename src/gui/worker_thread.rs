use image::{DynamicImage, ImageReader, Rgba};
use pxls::{dither_original_with_palette, get_palette, DistanceAlgorithm, OutputSettings, PaletteSettings};
use rfd::FileDialog;
use std::env::current_dir;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

pub enum ThreadRequest {
    GetInputImage,
    GetOutputImage(usize),
    RenderPalette {
        input: Arc<DynamicImage>,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
        progress_tx: Sender<(u32, u32)>
    },
    RenderOutput {
        input: Arc<DynamicImage>,
        palette: Vec<Rgba<u8>>,
        palette_settings: PaletteSettings,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        progress_tx: Sender<(u32, u32)>
    },
}

pub enum ThreadResult {
    ReadInFile(Arc<DynamicImage>),
    GotDestination(PathBuf, usize),
    RenderedPalette {
        input: Arc<DynamicImage>,
        palette: Vec<Rgba<u8>>,
        palette_settings: PaletteSettings,
    },
    RenderedImage {
        input: Arc<DynamicImage>,
        palette: Vec<Rgba<u8>>,
        output: DynamicImage,
        settings: (PaletteSettings, OutputSettings, DistanceAlgorithm),
    },
}

#[allow(clippy::type_complexity)]
pub fn start_worker_thread() -> (
    JoinHandle<()>,
    Sender<ThreadRequest>,
    Receiver<ThreadResult>,
    Arc<AtomicBool>,
) {
    let (req_tx, req_rx) = channel();
    let (res_tx, res_rx) = channel();
    let should_stop = Arc::new(AtomicBool::new(false));
    let ret_should_stop = should_stop.clone();

    let handle = std::thread::spawn(move || loop {
        if should_stop.load(Ordering::Relaxed) {
            break;
        }

        for req in req_rx.try_iter() {
            match req {
                ThreadRequest::GetInputImage => {
                    if let Some(file) = FileDialog::new()
                        .set_directory(current_dir().unwrap_or_else(|_| "/".into()))
                        .pick_file()
                    {
                        match ImageReader::open(file) {
                            Ok(img) => match img.decode() {
                                Ok(img) => {
                                    res_tx.send(ThreadResult::ReadInFile(Arc::new(img))).unwrap();
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
                    progress_tx,
                } => {
                    let palette = get_palette(
                        &input,
                        palette_settings,
                        distance_algorithm,
                        &progress_tx,
                        should_stop.clone(),
                    );

                    res_tx
                        .send(ThreadResult::RenderedPalette {
                            input,
                            palette,
                            palette_settings: palette_settings,
                        })
                        .unwrap();
                }
                ThreadRequest::RenderOutput {
                    input,
                    palette,
                    palette_settings,
                    mut output_settings,
                    distance_algorithm,
                    progress_tx
                } => {
                    let original_scale = output_settings.scale_output_to_original;
                    output_settings.scale_output_to_original = false;
                    let output = dither_original_with_palette(
                        &input,
                        &palette,
                        distance_algorithm,
                        output_settings,
                        &progress_tx,
                        should_stop.clone(),
                    );
                    output_settings.scale_output_to_original = original_scale;

                    res_tx
                        .send(ThreadResult::RenderedImage {
                            input,
                            palette,
                            output,
                            settings: (palette_settings, output_settings, distance_algorithm),
                        })
                        .unwrap();
                }
                ThreadRequest::GetOutputImage(index) => {
                    if let Some(file) = FileDialog::new()
                        .add_filter("Image Files", &["png", "jpg"])
                        .set_directory(current_dir().unwrap_or_else(|_| "/".into()))
                        .save_file()
                    {
                        res_tx
                            .send(ThreadResult::GotDestination(file, index))
                            .unwrap();
                    }
                }
            }
        }
    });

    (handle, req_tx, res_rx, ret_should_stop)
}
