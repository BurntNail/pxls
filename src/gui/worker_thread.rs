use image::{DynamicImage, ImageReader, Rgba};
use pxls::{
    dither_original_with_palette, get_palette, pixel_operations::rgb_to_hsv, DistanceAlgorithm,
    OutputSettings, PaletteSettings,
};
use rfd::FileDialog;
use std::{
    env::current_dir,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
};

pub enum ThreadRequest {
    GetInputImage,
    GetOutputImage(usize),
    RenderPalette {
        input: Arc<DynamicImage>,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
        progress_tx: Sender<(u32, u32)>,
    },
    RenderOutput {
        input: Arc<DynamicImage>,
        palette: Arc<[Rgba<u8>]>,
        palette_settings: PaletteSettings,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        progress_tx: Sender<(u32, u32)>,
    },
}

pub enum ThreadResult {
    ReadInFile(PathBuf, Arc<DynamicImage>),
    GotDestination {
        file: PathBuf,
        index: usize,
        save_dir: PathBuf,
    },
    RenderedPalette {
        input: Arc<DynamicImage>,
        palette: Arc<[Rgba<u8>]>,
        palette_settings: PaletteSettings,
    },
    RenderedImage {
        input: Arc<DynamicImage>,
        palette: Arc<[Rgba<u8>]>,
        output: DynamicImage,
        settings: (PaletteSettings, OutputSettings, DistanceAlgorithm),
    },
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_lines)]
pub fn start_worker_thread(
    (last_start_dir, last_save_dir): (Option<PathBuf>, Option<PathBuf>),
) -> (
    JoinHandle<()>,
    Sender<ThreadRequest>,
    Receiver<ThreadResult>,
    Arc<AtomicBool>,
) {
    let (req_tx, req_rx) = channel();
    let (res_tx, res_rx) = channel();
    let should_stop = Arc::new(AtomicBool::new(false));
    let ret_should_stop = should_stop.clone();

    let handle = std::thread::spawn(move || {
        let mut last_start_dir =
            last_start_dir.unwrap_or_else(|| current_dir().unwrap_or_else(|_| "/".into()));
        let mut last_save_dir = last_save_dir.unwrap_or_else(|| last_start_dir.clone());

        loop {
            if should_stop.load(Ordering::Relaxed) {
                break;
            }

            for req in req_rx.try_iter() {
                match req {
                    ThreadRequest::GetInputImage => {
                        if let Some(file) =
                            FileDialog::new().set_directory(&last_start_dir).pick_file()
                        {
                            if let Some(parent) = file.parent() {
                                last_start_dir = parent.to_path_buf();
                            }
                            match ImageReader::open(file) {
                                Ok(img) => match img.decode() {
                                    Ok(img) => {
                                        res_tx
                                            .send(ThreadResult::ReadInFile(
                                                last_start_dir.clone(),
                                                Arc::new(img),
                                            ))
                                            .unwrap();
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
                        let mut palette = get_palette(
                            &input,
                            palette_settings,
                            distance_algorithm,
                            &progress_tx,
                            should_stop.clone(),
                        );

                        palette.sort_by_cached_key(|x| rgb_to_hsv(*x)[0]);

                        res_tx
                            .send(ThreadResult::RenderedPalette {
                                input,
                                palette: palette.into(),
                                palette_settings,
                            })
                            .unwrap();
                    }
                    ThreadRequest::RenderOutput {
                        input,
                        palette,
                        palette_settings,
                        output_settings,
                        distance_algorithm,
                        progress_tx,
                    } => {
                        let output = dither_original_with_palette(
                            &input,
                            &palette,
                            distance_algorithm,
                            OutputSettings {
                                scale_output_to_original: false,
                                ..output_settings
                            },
                            &progress_tx,
                            should_stop.clone(),
                        );

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
                            .set_directory(&last_save_dir)
                            .save_file()
                        {
                            if let Some(parent) = file.parent() {
                                last_save_dir = parent.to_path_buf();
                            }

                            res_tx
                                .send(ThreadResult::GotDestination {
                                    file,
                                    index,
                                    save_dir: last_save_dir.clone(),
                                })
                                .unwrap();
                        }
                    }
                }
            }
        }
    });

    (handle, req_tx, res_rx, ret_should_stop)
}
