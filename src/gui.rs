use crate::gui::worker_thread::{
    start_worker_thread, OutputSettings, PaletteSettings, ThreadRequest, ThreadResult,
};
use crate::logic::{DistanceAlgorithm, ALL_ALGOS};
use eframe::{CreationContext, Frame, NativeOptions};
use egui::panel::TopBottomSide;
use egui::{pos2, Color32, ColorImage, Context, Rect, TextureHandle, TextureId, TextureOptions};
use egui_extras::install_image_loaders;
use image::{DynamicImage, GenericImageView, Pixel, Rgba};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

mod worker_thread;

pub fn gui_main() {
    let native_options = NativeOptions::default();

    if let Err(e) = eframe::run_native(
        "Pxls",
        native_options,
        Box::new(|cc| Ok(Box::new(PxlsApp::new(cc)))),
    ) {
        eprintln!("Error running eframe: {e:?}");
    }
}

enum RenderStage {
    Nothing,
    ReadInImage,
    WithPalette,
    RenderedImage {
        input: DynamicImage,
        palette: Vec<Rgba<u8>>,
        output: DynamicImage,
        handle: TextureHandle,
    },
}

struct PhotoBeingEdited {
    stage: RenderStage,
    worker_handle: Option<JoinHandle<()>>,
    worker_should_stop: Arc<AtomicBool>,
    requests_tx: Sender<ThreadRequest>,
    results_rx: Receiver<ThreadResult>,
    texture_options: TextureOptions,
    output_file_name: String,
}

impl PhotoBeingEdited {
    pub fn new() -> Self {
        let (worker_handle, requests_tx, results_rx, worker_should_stop) = start_worker_thread();

        Self {
            stage: RenderStage::Nothing,
            worker_handle: Some(worker_handle),
            requests_tx,
            results_rx,
            worker_should_stop,
            texture_options: TextureOptions::NEAREST,
            output_file_name: "output.jpg".to_string(),
        }
    }

    pub fn pick_new_file(&mut self) {
        self.requests_tx.send(ThreadRequest::GetFile).unwrap();
        self.stage = RenderStage::Nothing;
    }

    pub fn process_thread_updates(
        &mut self,
        palette_settings: PaletteSettings,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        ctx: &Context,
    ) {
        for update in self.results_rx.try_iter() {
            match update {
                ThreadResult::ReadInFile(input) => {
                    self.stage = RenderStage::ReadInImage;
                    self.requests_tx
                        .send(ThreadRequest::RenderPalette {
                            input,
                            palette_settings,
                            distance_algorithm,
                        })
                        .unwrap(); //TODO: fix all these unwraps
                }
                ThreadResult::RenderedPalette(input, palette) => {
                    self.stage = RenderStage::WithPalette;
                    self.requests_tx
                        .send(ThreadRequest::RenderOutput {
                            input,
                            palette,
                            output_settings,
                            distance_algorithm,
                        })
                        .unwrap();
                }
                ThreadResult::RenderedImage {
                    input,
                    palette,
                    output,
                } => {
                    let handle = ctx.load_texture(
                        "my-img",
                        Self::color_image_from_dynamic_image(&output),
                        self.texture_options,
                    );

                    self.stage = RenderStage::RenderedImage {
                        input,
                        palette,
                        output,
                        handle,
                    };
                }
            }
        }
    }

    pub fn change_palette_settings_or_algo(
        &mut self,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
    ) {
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::ReadInImage);
        if let RenderStage::RenderedImage { input, .. } = originally_contained {
            self.requests_tx
                .send(ThreadRequest::RenderPalette {
                    input,
                    palette_settings,
                    distance_algorithm,
                })
                .unwrap();
        } else {
            self.stage = originally_contained;
        }
    }

    pub fn change_output_settings(
        &mut self,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
    ) {
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::ReadInImage);
        if let RenderStage::RenderedImage { input, palette, .. } = originally_contained {
            self.requests_tx
                .send(ThreadRequest::RenderOutput {
                    input,
                    palette,
                    output_settings,
                    distance_algorithm,
                })
                .unwrap();
        } else {
            self.stage = originally_contained;
        }
    }

    pub const fn is_ready_for_more_input(&self) -> bool {
        matches!(
            self.stage,
            RenderStage::RenderedImage { .. } | RenderStage::Nothing
        )
    }

    fn color_image_from_dynamic_image(img: &DynamicImage) -> ColorImage {
        let (width, height) = (img.width() as _, img.height() as _);
        let mut pixels = Vec::with_capacity(width * height * 3);

        for y in 0..img.height() {
            for x in 0..img.width() {
                let rgb = img.get_pixel(x as _, y as _).to_rgb().0;
                pixels.extend_from_slice(rgb.as_slice());
            }
        }

        ColorImage::from_rgb([width, height], &pixels)
    }
}

#[derive(Clone, Debug)]
struct SettingsBuffers {
    chunks_per_dimension: String,
    closeness_threshold: String,
    output_px_size: String,
    dithering_factor: String,
}

struct PxlsApp {
    current: PhotoBeingEdited,
    distance_algorithm: DistanceAlgorithm,
    palette_settings: PaletteSettings,
    output_settings: OutputSettings,
    setting_change_buffers: SettingsBuffers,
    needs_to_refresh_palette: bool,
    needs_to_refresh_output: bool,
    auto_update: bool,
}

impl PxlsApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        install_image_loaders(&cc.egui_ctx);

        let palette_settings = PaletteSettings::default();
        let output_settings = OutputSettings::default();

        Self {
            current: PhotoBeingEdited::new(),
            distance_algorithm: DistanceAlgorithm::Euclidean,
            palette_settings,
            output_settings,
            auto_update: false,
            setting_change_buffers: SettingsBuffers {
                chunks_per_dimension: palette_settings.chunks_per_dimension.to_string(),
                closeness_threshold: palette_settings.closeness_threshold.to_string(),
                output_px_size: output_settings.output_px_size.to_string(),
                dithering_factor: output_settings.dithering_factor.to_string(),
            },
            needs_to_refresh_output: false,
            needs_to_refresh_palette: false,
        }
    }
}

impl eframe::App for PxlsApp {
    #[allow(clippy::too_many_lines)]
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.current.process_thread_updates(
            self.palette_settings,
            self.output_settings,
            self.distance_algorithm,
            ctx,
        );

        egui::TopBottomPanel::new(TopBottomSide::Top, "top_panel").show(ctx, |ui| {
            if !self.current.is_ready_for_more_input() {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if ui.button("Select File").clicked() {
                        self.current.pick_new_file();
                    }

                    ui.checkbox(&mut self.auto_update, "Auto-Update");
                });

                ui.vertical(|ui| {
                    ui.label("Distance Algorithm:");

                    let current = self.distance_algorithm;
                    for possibility in ALL_ALGOS {
                        ui.radio_value(
                            &mut self.distance_algorithm,
                            possibility,
                            possibility.to_str(),
                        );
                    }

                    if current != self.distance_algorithm {
                        self.needs_to_refresh_palette = true;
                    }
                });

                ui.separator();

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Chunks per Dimension: ");
                        ui.text_edit_singleline(
                            &mut self.setting_change_buffers.chunks_per_dimension,
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Closeness Threshold: ");
                        ui.text_edit_singleline(
                            &mut self.setting_change_buffers.closeness_threshold,
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Virtual Pixel Size: ");
                        ui.text_edit_singleline(&mut self.setting_change_buffers.output_px_size);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Dithering Factor: ");
                        ui.text_edit_singleline(&mut self.setting_change_buffers.dithering_factor);
                    });
                });

                if let Ok(new_chunks_per_dimension) =
                    self.setting_change_buffers.chunks_per_dimension.parse()
                {
                    if new_chunks_per_dimension != self.palette_settings.chunks_per_dimension {
                        self.needs_to_refresh_palette = true;
                        self.palette_settings.chunks_per_dimension = new_chunks_per_dimension;
                    }
                }
                if let Ok(new_closeness_threshold) =
                    self.setting_change_buffers.closeness_threshold.parse()
                {
                    if new_closeness_threshold != self.palette_settings.closeness_threshold {
                        self.needs_to_refresh_palette = true;
                        self.palette_settings.closeness_threshold = new_closeness_threshold;
                    }
                }
                if let Ok(new_output_px_size) = self.setting_change_buffers.output_px_size.parse() {
                    if new_output_px_size != self.output_settings.output_px_size {
                        self.needs_to_refresh_output = true;
                        self.output_settings.output_px_size = new_output_px_size;
                    }
                }
                if let Ok(new_dithering_factor) =
                    self.setting_change_buffers.dithering_factor.parse()
                {
                    if new_dithering_factor != self.output_settings.dithering_factor {
                        self.needs_to_refresh_output = true;
                        self.output_settings.dithering_factor = new_dithering_factor;
                    }
                }

                if self.needs_to_refresh_palette || self.needs_to_refresh_output {
                    let mut needs_to_update = self.auto_update;
                    if !needs_to_update {
                        ui.separator();
                        needs_to_update = ui.button("Update").clicked();
                    }

                    if needs_to_update {
                        if self.needs_to_refresh_palette {
                            self.current.change_palette_settings_or_algo(
                                self.palette_settings,
                                self.distance_algorithm,
                            );
                        } else if self.needs_to_refresh_output {
                            self.current.change_output_settings(
                                self.output_settings,
                                self.distance_algorithm,
                            );
                        }

                        self.needs_to_refresh_palette = false;
                        self.needs_to_refresh_output = false;
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match &self.current.stage {
                RenderStage::Nothing => {
                    ui.centered_and_justified(|ui| {
                        ui.label("Pick a file!");
                    });
                }
                RenderStage::ReadInImage => {
                    ui.centered_and_justified(|ui| {
                        ui.label("Creating palette...");
                    });
                }
                RenderStage::WithPalette => {
                    ui.centered_and_justified(|ui| {
                        ui.label("Converting and dithering...");
                    });
                }
                RenderStage::RenderedImage { input, handle, .. } => {
                    let texture_id = TextureId::from(handle);
                    let available_width = ui.available_width();
                    let available_height = ui.available_height();
                    let available_aspect = available_width / available_height;

                    let (img_width, img_height) = (input.width() as f32, input.height() as f32);
                    let img_aspect = img_width / img_height;

                    let (uv_x, uv_y) = if available_aspect > img_aspect {
                        (available_aspect / img_aspect, 1.0)
                    } else {
                        (1.0, img_aspect / available_aspect)
                    }; //TODO: now that we've got this fun scaling, the image doesn't need to be weirdly big

                    let uv = Rect {
                        min: pos2(0.0, 0.0),
                        max: pos2(uv_x, uv_y),
                    };

                    ui.painter().image(
                        texture_id,
                        ui.available_rect_before_wrap(),
                        uv,
                        Color32::WHITE,
                    );
                }
            }
        });

        if let RenderStage::RenderedImage { output, .. } = &self.current.stage {
            egui::TopBottomPanel::new(TopBottomSide::Bottom, "bottom-panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Output File Name: ");
                    ui.text_edit_singleline(&mut self.current.output_file_name);

                    ui.separator();

                    if ui.button("Save").clicked() {
                        if let Err(e) = output.save(&self.current.output_file_name) {
                            eprintln!("Error saving file: {e:?}");
                        }
                    }
                })
            });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.current
            .worker_should_stop
            .store(true, Ordering::Relaxed);
        if let Some(handle) = self.current.worker_handle.take() {
            if handle.join().is_err() {
                eprintln!("Error joining thread");
            }
        }
    }
}
