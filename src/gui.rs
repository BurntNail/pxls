use crate::gui::worker_thread::{start_worker_thread, ThreadRequest, ThreadResult};
use eframe::{CreationContext, Frame, NativeOptions};
use egui::panel::TopBottomSide;
use egui::{
    pos2, Color32, ColorImage, Context, ProgressBar, Rect, Slider, TextureHandle, TextureId,
    TextureOptions, Widget,
};
use egui_extras::install_image_loaders;
use image::{DynamicImage, GenericImageView, Pixel, Rgba};
use pxls::{DistanceAlgorithm, OutputSettings, PaletteSettings, ALL_ALGOS};
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
    DisplayingImage(usize),
}

#[derive(Clone)]
struct RenderedImage {
    input: DynamicImage,
    palette: Vec<Rgba<u8>>,
    output: DynamicImage,
    handle: TextureHandle,
    settings: (PaletteSettings, OutputSettings, DistanceAlgorithm),
}

struct PhotoBeingEdited {
    stage: RenderStage,
    progress_rx: Receiver<(u32, u32)>,
    last_progress_received: (u32, u32),
    worker_handle: Option<JoinHandle<()>>,
    worker_should_stop: Arc<AtomicBool>,
    requests_tx: Sender<ThreadRequest>,
    results_rx: Receiver<ThreadResult>,
    texture_options: TextureOptions,
    image_history: Vec<RenderedImage>,
}

impl PhotoBeingEdited {
    pub fn new() -> Self {
        let (worker_handle, requests_tx, results_rx, progress_rx, worker_should_stop) =
            start_worker_thread();

        Self {
            stage: RenderStage::Nothing,
            progress_rx,
            last_progress_received: (0, 1),
            worker_handle: Some(worker_handle),
            requests_tx,
            results_rx,
            worker_should_stop,
            texture_options: TextureOptions::NEAREST,
            image_history: vec![],
        }
    }

    pub fn pick_new_input(&mut self) {
        self.requests_tx.send(ThreadRequest::GetInputImage).unwrap();
        self.stage = RenderStage::Nothing;
    }

    pub fn save_file(&self, index: usize) {
        self.requests_tx
            .send(ThreadRequest::GetOutputImage(index))
            .unwrap();
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
                    self.last_progress_received = (0, 1);
                }
                ThreadResult::RenderedPalette(input, palette, palette_settings) => {
                    self.stage = RenderStage::WithPalette;
                    self.requests_tx
                        .send(ThreadRequest::RenderOutput {
                            input,
                            palette,
                            palette_settings,
                            output_settings,
                            distance_algorithm,
                        })
                        .unwrap();
                    self.last_progress_received = (0, 1);
                }
                ThreadResult::RenderedImage {
                    input,
                    palette,
                    output,
                    settings,
                } => {
                    let handle = ctx.load_texture(
                        "my-img",
                        Self::color_image_from_dynamic_image(&output),
                        self.texture_options,
                    );
                    let ri = RenderedImage {
                        input,
                        palette,
                        output,
                        handle,
                        settings,
                    };

                    self.image_history.push(ri.clone());
                    self.stage = RenderStage::DisplayingImage(self.image_history.len() - 1);
                    self.last_progress_received = (0, 1);
                }
                ThreadResult::GotDestination(dst, index) => {
                    let output = &self.image_history[index];
                    if let Err(e) = output.output.save(dst) {
                        eprintln!("Error saving file: {e:?}");
                    }
                }
            }
        }

        for prog in self.progress_rx.try_iter() {
            self.last_progress_received = prog;
        }
    }

    pub fn change_palette_settings_or_algo(
        &mut self,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
    ) {
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::ReadInImage);
        if let RenderStage::DisplayingImage(idx) = originally_contained {
            let input = self.image_history[idx].input.clone();

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
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::WithPalette);
        if let RenderStage::DisplayingImage(index) = originally_contained {
            let ri = &self.image_history[index];

            self.requests_tx
                .send(ThreadRequest::RenderOutput {
                    input: ri.input.clone(),
                    palette: ri.palette.clone(),
                    palette_settings: ri.settings.0,
                    output_settings,
                    distance_algorithm,
                })
                .unwrap();
        } else {
            self.stage = originally_contained;
        }
    }

    #[inline]
    pub const fn is_ready_for_more_input(&self) -> bool {
        matches!(
            self.stage,
            RenderStage::DisplayingImage { .. } | RenderStage::Nothing
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
                dithering_factor: output_settings.dithering_likelihood.to_string(),
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
                        self.current.pick_new_input();
                    }

                    ui.checkbox(&mut self.auto_update, "Auto-Update");

                    if self.needs_to_refresh_palette || self.needs_to_refresh_output {
                        let mut needs_to_update = self.auto_update;
                        if !needs_to_update {
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

                ui.vertical(|ui| {
                    ui.label("Distance Algorithm:");

                    let current = self.distance_algorithm;
                    for possibility in ALL_ALGOS {
                        ui.radio_value(
                            &mut self.distance_algorithm,
                            *possibility,
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
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Virtual Pixel Size: ");

                        let old_px_size = self.output_settings.output_px_size;

                        //make sure we don't get images that are too big to display. this is a pretty lazy solution, but i also can't see an alternative because we might not have an image yet lol
                        let min = self.output_settings.dithering_scale.ilog2() + 1;
                        ui.add(Slider::new(
                            &mut self.output_settings.output_px_size,
                            min..=10,
                        ));

                        if old_px_size != self.output_settings.output_px_size {
                            self.needs_to_refresh_output = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Dithering Factor: ");
                        ui.text_edit_singleline(&mut self.setting_change_buffers.dithering_factor);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Dithering Scale: ");

                        let old_ds = self.output_settings.dithering_scale;
                        ui.add(Slider::new(
                            &mut self.output_settings.dithering_scale,
                            1..=4,
                        ));

                        if old_ds != self.output_settings.dithering_scale {
                            self.needs_to_refresh_output = true;
                            self.output_settings.output_px_size =
                                (self.output_settings.dithering_scale.ilog2() + 1)
                                    .max(self.output_settings.output_px_size);
                        }
                    });

                    let old_output_scaling = self.output_settings.scale_output_to_original;
                    ui.checkbox(
                        &mut self.output_settings.scale_output_to_original,
                        "Preserve image size when saving",
                    );
                    if old_output_scaling != self.output_settings.scale_output_to_original {
                        self.needs_to_refresh_output = true;
                    }
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
                if let Ok(new_dithering_factor) =
                    self.setting_change_buffers.dithering_factor.parse()
                {
                    if new_dithering_factor != self.output_settings.dithering_likelihood {
                        self.needs_to_refresh_output = true;
                        self.output_settings.dithering_likelihood = new_dithering_factor;
                    }
                }
            });
        });

        if matches!(self.current.stage, RenderStage::DisplayingImage(_)) {
            egui::TopBottomPanel::new(TopBottomSide::Bottom, "bottom-panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    //have to do the weird ifs for mutability reasons
                    if let RenderStage::DisplayingImage(index) = &mut self.current.stage {
                        ui.label("History: ");
                        let previous = *index;

                        #[allow(clippy::range_minus_one)] //😔
                        ui.add(Slider::new(
                            index,
                            0..=(self.current.image_history.len() - 1),
                        ));

                        if previous != *index {
                            let (palette, output, distance) = self.current.image_history[*index].settings;
                            self.palette_settings = palette;
                            self.output_settings = output;
                            self.distance_algorithm = distance;
                        }
                    }

                    ui.separator();

                    if let RenderStage::DisplayingImage(index) = &self.current.stage {
                        if ui.button("Save").clicked() {
                            self.current.save_file(*index);
                        }
                    }
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            match &self.current.stage {
                RenderStage::Nothing => {
                    ui.centered_and_justified(|ui| {
                        ui.label("Pick a file!");
                    });
                }
                RenderStage::ReadInImage => {
                    ui.label("Creating palette...");

                    let (so_far, max) = self.current.last_progress_received;
                    ProgressBar::new((so_far as f32) / (max as f32))
                        .animate(true)
                        .show_percentage()
                        .ui(ui);
                }
                RenderStage::WithPalette => {
                    ui.label("Converting and dithering...");

                    let (so_far, max) = self.current.last_progress_received;
                    ProgressBar::new((so_far as f32) / (max as f32))
                        .animate(true)
                        .show_percentage()
                        .ui(ui);
                }
                RenderStage::DisplayingImage(index) => {
                    let RenderedImage {
                        output,
                        handle,
                        ..
                    } = &self.current.image_history[*index];

                    let texture_id = TextureId::from(handle);

                    let uv = Rect {
                        min: pos2(0.0, 0.0),
                        max: pos2(1.0, 1.0),
                    }; //TODO: pan & zoom?

                    let mut rect = ui.available_rect_before_wrap();
                    {
                        let (img_width, img_height) =
                            (output.width() as f32, output.height() as f32);
                        let img_aspect = img_width / img_height;
                        let available_aspect = rect.width() / rect.height();

                        let (sf_x, sf_y) = if available_aspect > img_aspect {
                            (available_aspect / img_aspect, 1.0)
                        } else {
                            (1.0, img_aspect / available_aspect)
                        };

                        rect.max.x = rect.min.x + rect.width() / sf_x;
                        rect.max.y = rect.min.y + rect.height() / sf_y;
                    }

                    ui.painter().image(texture_id, rect, uv, Color32::WHITE);
                }
            }
        });
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
