use crate::gui::worker_thread::{start_worker_thread, ThreadRequest, ThreadResult};
use eframe::{CreationContext, Frame, NativeOptions, Storage};
use egui::{
    panel::TopBottomSide, pos2, Color32, ColorImage, Context, Grid, ProgressBar, Rect, Sense,
    Slider, TextureHandle, TextureId, TextureOptions, Widget,
};
use image::{DynamicImage, GenericImageView, Pixel, Rgba};
use pxls::{pixel_perfect_scale, DistanceAlgorithm, OutputSettings, PaletteSettings, ALL_ALGOS};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
};

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
    CreatingPalette {
        last_progress: (u32, u32),
        progress_rx: Receiver<(u32, u32)>,
    },
    CreatingOutput {
        palette_used: Arc<[Rgba<u8>]>,
        last_progress: (u32, u32),
        progress_rx: Receiver<(u32, u32)>,
    },
    DisplayingImage(usize),
}

#[derive(Clone)]
struct RenderedImage {
    input: Arc<DynamicImage>,
    palette: Arc<[Rgba<u8>]>,
    output: DynamicImage,
    handle: TextureHandle,
    settings: (PaletteSettings, OutputSettings, DistanceAlgorithm),
}

struct RenderedPalette {
    input: (Arc<[Rgba<u8>]>, Rect),
    dimensions: [usize; 2],
    handle: TextureHandle,
}

struct PhotoBeingEdited {
    stage: RenderStage,
    worker_handle: Option<JoinHandle<()>>,
    last_start_save_dirs: (Option<PathBuf>, Option<PathBuf>),
    worker_should_stop: Arc<AtomicBool>,
    requests_tx: Sender<ThreadRequest>,
    results_rx: Receiver<ThreadResult>,
    texture_options: TextureOptions,
    image_history: Vec<RenderedImage>,
}

impl PhotoBeingEdited {
    pub fn new(last_start_save_dirs: (Option<PathBuf>, Option<PathBuf>)) -> Self {
        let (worker_handle, requests_tx, results_rx, worker_should_stop) =
            start_worker_thread(last_start_save_dirs.clone());

        Self {
            stage: RenderStage::Nothing,
            worker_handle: Some(worker_handle),
            last_start_save_dirs,
            requests_tx,
            results_rx,
            worker_should_stop,
            texture_options: TextureOptions::NEAREST,
            image_history: vec![],
        }
    }

    pub fn pick_new_input(&self) {
        self.requests_tx.send(ThreadRequest::GetInputImage).unwrap();
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
                ThreadResult::ReadInFile(start_dir, input) => {
                    let (progress_tx, progress_rx) = channel();
                    self.stage = RenderStage::CreatingPalette {
                        progress_rx,
                        last_progress: (0, 1),
                    };
                    self.requests_tx
                        .send(ThreadRequest::RenderPalette {
                            input,
                            palette_settings,
                            distance_algorithm,
                            progress_tx,
                        })
                        .unwrap();

                    self.last_start_save_dirs.0 = Some(start_dir);
                }
                ThreadResult::RenderedPalette {
                    input,
                    palette,
                    palette_settings,
                } => {
                    let (progress_tx, progress_rx) = channel();
                    self.stage = RenderStage::CreatingOutput {
                        palette_used: palette.clone(),
                        progress_rx,
                        last_progress: (0, 1),
                    };
                    self.requests_tx
                        .send(ThreadRequest::RenderOutput {
                            input,
                            palette,
                            palette_settings,
                            output_settings,
                            distance_algorithm,
                            progress_tx,
                        })
                        .unwrap();
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
                }
                ThreadResult::GotDestination {
                    file,
                    index,
                    save_dir,
                } => {
                    if let Some(output) = self.image_history.get(index) {
                        let scaled = pixel_perfect_scale(output_settings, &output.output);

                        if let Err(e) = scaled.save(file) {
                            eprintln!("Error saving file: {e:?}");
                        }
                    }

                    self.last_start_save_dirs.1 = Some(save_dir);
                }
            }
        }

        match &mut self.stage {
            RenderStage::CreatingOutput {
                palette_used: _,
                last_progress,
                progress_rx,
            }
            | RenderStage::CreatingPalette {
                last_progress,
                progress_rx,
            } => {
                for prog in progress_rx.try_iter() {
                    *last_progress = prog;
                }
            }
            _ => {}
        }
    }

    pub fn change_palette_settings_or_algo(
        &mut self,
        palette_settings: PaletteSettings,
        distance_algorithm: DistanceAlgorithm,
    ) {
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::Nothing);
        if let RenderStage::DisplayingImage(idx) = originally_contained {
            let input = self.image_history[idx].input.clone();
            let (progress_tx, progress_rx) = channel();

            self.requests_tx
                .send(ThreadRequest::RenderPalette {
                    input,
                    palette_settings,
                    distance_algorithm,
                    progress_tx,
                })
                .unwrap();

            self.stage = RenderStage::CreatingPalette {
                progress_rx,
                last_progress: (0, 1),
            }
        } else {
            self.stage = originally_contained;
        }
    }

    pub fn change_output_settings(
        &mut self,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
    ) {
        let originally_contained = std::mem::replace(&mut self.stage, RenderStage::Nothing);
        if let RenderStage::DisplayingImage(index) = originally_contained {
            let (progress_tx, progress_rx) = channel();
            let ri = &self.image_history[index];

            self.requests_tx
                .send(ThreadRequest::RenderOutput {
                    input: ri.input.clone(),
                    palette: ri.palette.clone(),
                    palette_settings: ri.settings.0,
                    output_settings,
                    distance_algorithm,
                    progress_tx,
                })
                .unwrap();

            self.stage = RenderStage::CreatingOutput {
                palette_used: ri.palette.clone(),
                progress_rx,
                last_progress: (0, 1),
            }
        } else {
            self.stage = originally_contained;
        }
    }

    fn color_image_from_dynamic_image(img: &DynamicImage) -> ColorImage {
        let size = [img.width() as _, img.height() as _];
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                ColorImage::from_rgb(size, rgb.as_flat_samples().as_slice())
            }
            DynamicImage::ImageRgba8(rgba) => {
                ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice())
            }
            _ => {
                let mut pixels = Vec::with_capacity(size[0] * size[1] * 3);

                for y in 0..img.height() {
                    for x in 0..img.width() {
                        let rgb = img.get_pixel(x as _, y as _).to_rgb().0;
                        pixels.extend_from_slice(rgb.as_slice());
                    }
                }

                ColorImage::from_rgb(size, &pixels)
            }
        }
    }
}

struct PxlsApp {
    current: PhotoBeingEdited,
    show_palette: Option<RenderedPalette>,
    distance_algorithm: DistanceAlgorithm,
    palette_settings: PaletteSettings,
    output_settings: OutputSettings,
    needs_to_refresh_palette: bool,
    needs_to_refresh_output: bool,
    auto_update: bool,
}

impl PxlsApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        const FALLBACK: (Option<PathBuf>, Option<PathBuf>) = (None, None);
        let start_and_save_dirs = cc.storage.map_or(FALLBACK, |storage| {
            storage
                .get_string("start_and_save_dirs")
                .map_or(FALLBACK, |sered| {
                    serde_json::from_str(&sered).unwrap_or(FALLBACK)
                })
        });

        Self {
            current: PhotoBeingEdited::new(start_and_save_dirs),
            show_palette: None,
            distance_algorithm: DistanceAlgorithm::Euclidean,
            palette_settings: PaletteSettings::default(),
            output_settings: OutputSettings::default(),
            auto_update: true,
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
            if !matches!(
                &self.current.stage,
                RenderStage::Nothing | RenderStage::DisplayingImage(_)
            ) {
                ui.disable();
            }

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if ui.button("Select File").clicked() {
                        self.current.pick_new_input();
                    }

                    ui.checkbox(&mut self.auto_update, "Auto-Update");

                    if self.needs_to_refresh_output || self.needs_to_refresh_palette {
                        if let RenderStage::DisplayingImage(index) = &mut self.current.stage {
                            let mut needs_to_update = self.auto_update;
                            if !needs_to_update {
                                needs_to_update = ui.button("Update").clicked();
                            }

                            if needs_to_update {
                                let mut found = false;
                                for (
                                    i,
                                    RenderedImage {
                                        input,
                                        settings: (palette, output, distance),
                                        ..
                                    },
                                ) in self.current.image_history.iter().enumerate()
                                {
                                    //hopefully short-circuiting should ensure that the input is compared last :)
                                    if self.distance_algorithm == *distance
                                        && self.palette_settings == *palette
                                        && self.output_settings == *output
                                        && &self.current.image_history[*index].input == input
                                    {
                                        *index = i;
                                        found = true;
                                        break;
                                    }
                                }

                                if !found {
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
                                }

                                self.needs_to_refresh_palette = false;
                                self.needs_to_refresh_output = false;
                            }
                        }
                    }

                    let old_output_scaling = self.output_settings.scale_output_to_original;
                    ui.checkbox(
                        &mut self.output_settings.scale_output_to_original,
                        "Preserve image size when saving",
                    );
                    if old_output_scaling != self.output_settings.scale_output_to_original {
                        self.needs_to_refresh_output = true;
                    }
                });

                ui.separator();

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
                    Grid::new("settings").show(ui, |ui| {
                        {
                            ui.label("Chunks per Dimension: ");
                            let old_cpd = self.palette_settings.chunks_per_dimension;
                            ui.add(
                                Slider::new(
                                    &mut self.palette_settings.chunks_per_dimension,
                                    1..=10_000,
                                )
                                .logarithmic(true),
                            );

                            if self.palette_settings.chunks_per_dimension != old_cpd {
                                self.needs_to_refresh_palette = true;
                            }

                            ui.end_row();
                        }
                        {
                            ui.label("Closeness Threshold: ");

                            let old_ct = self.palette_settings.closeness_threshold;
                            ui.add(
                                Slider::new(
                                    &mut self.palette_settings.closeness_threshold,
                                    0..=255,
                                )
                                .logarithmic(true),
                            );

                            if self.palette_settings.closeness_threshold != old_ct {
                                self.needs_to_refresh_palette = true;
                            }

                            ui.end_row();
                        }
                        {
                            ui.separator();
                            ui.end_row();
                        }
                        {
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

                            ui.end_row();
                        }
                        {
                            ui.label("Dithering Factor: ");

                            let old_dl = self.output_settings.dithering_likelihood;
                            ui.add_enabled(
                                self.output_settings.dithering_scale > 1,
                                Slider::new(&mut self.output_settings.dithering_likelihood, 1..=5),
                            );

                            if old_dl != self.output_settings.dithering_likelihood {
                                self.needs_to_refresh_output = true;
                            }

                            ui.end_row();
                        }
                        {
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

                            ui.end_row();
                        }

                        {
                            let palette_len = match &self.current.stage {
                                RenderStage::DisplayingImage(index) => {
                                    Some(self.current.image_history[*index].palette.len())
                                }
                                RenderStage::CreatingOutput { palette_used, .. } => {
                                    Some(palette_used.len())
                                }
                                _ => None,
                            };
                            if let Some(palette_len) = palette_len {
                                ui.separator();
                                ui.end_row();

                                ui.label("Current Palette Size:");
                                ui.label(palette_len.to_string());
                                ui.end_row();
                            }
                        }
                    });
                });

                let palette: Option<Arc<[Rgba<u8>]>> = match &self.current.stage {
                    RenderStage::DisplayingImage(index) => {
                        Some(self.current.image_history[*index].palette.clone())
                    }
                    RenderStage::CreatingOutput { palette_used, .. } => Some(palette_used.clone()),
                    _ => None,
                };
                if let Some(palette) = palette {
                    let available_rect = ui.available_rect_before_wrap();

                    let palette_to_show = {
                        match self.show_palette.as_ref() {
                            Some(old_palette)
                                if Arc::ptr_eq(&old_palette.input.0, &palette)
                                    && old_palette.input.1 == available_rect =>
                            {
                                old_palette
                            }
                            _ => {
                                let (horizontal_no_colours, vertical_no_colours) = {
                                    let palette_len = palette.len() as f32;
                                    let ratio = available_rect.width() / available_rect.height();

                                    let vertical_no_colours =
                                        (palette_len / ratio).sqrt().floor().max(1.0);
                                    let horizontal_no_colours =
                                        (palette_len / vertical_no_colours).ceil();

                                    (horizontal_no_colours, vertical_no_colours)
                                };
                                #[allow(clippy::cast_sign_loss)]
                                let (image_width, image_height) =
                                    (horizontal_no_colours as usize, vertical_no_colours as usize);

                                let mut palette_index = 0;
                                let mut color_image = ColorImage::new(
                                    [image_width, image_height],
                                    Color32::TRANSPARENT,
                                );

                                'outer: for row in 0..image_height {
                                    for column in 0..image_width {
                                        let [r, g, b] = palette[palette_index].to_rgb().0;
                                        color_image[(column, row)] = Color32::from_rgb(r, g, b);

                                        palette_index += 1;
                                        if palette_index >= palette.len() {
                                            break 'outer;
                                        }
                                    }
                                }

                                let dimensions = [color_image.width(), color_image.height()];
                                let handle = ctx.load_texture(
                                    "my-palette",
                                    color_image,
                                    self.current.texture_options,
                                );

                                //yes i could chuck some unsafe in here, but if LLVM doesn't catch this one i'll be VERY surprised
                                self.show_palette = Some(RenderedPalette {
                                    input: (palette, available_rect),
                                    dimensions,
                                    handle,
                                });
                                self.show_palette.as_ref().unwrap()
                            }
                        }
                    };

                    let _ = ui.allocate_rect(available_rect, Sense::hover()); //allocate to ensure we don't draw anything on top :)
                    let painter = ui.painter();

                    let display_rect = {
                        let (horizontal_no_colours, vertical_no_colours) = (
                            (palette_to_show.dimensions[0] as f32),
                            (palette_to_show.dimensions[1] as f32),
                        );

                        let cell_width = available_rect.width() / horizontal_no_colours;
                        let cell_height = available_rect.height() / vertical_no_colours;

                        let cell_size = cell_width.min(cell_height);

                        //mul by horizontal_no_colours to get the width taken up by the palette, then subtract that from the available width to get the buffer, then div by 2 to get the left buffer space
                        let start_x = available_rect.min.x
                            + cell_size.mul_add(-horizontal_no_colours, available_rect.width())
                                / 2.0;
                        let start_y = available_rect.min.y
                            + cell_size.mul_add(-vertical_no_colours, available_rect.height())
                                / 2.0;

                        Rect {
                            min: pos2(start_x, start_y),
                            max: pos2(
                                horizontal_no_colours.mul_add(cell_size, start_x),
                                vertical_no_colours.mul_add(cell_size, start_y),
                            ),
                        }
                    };

                    let texid = TextureId::from(&palette_to_show.handle);

                    painter.image(
                        texid,
                        display_rect,
                        Rect {
                            min: pos2(0.0, 0.0),
                            max: pos2(1.0, 1.0),
                        },
                        Color32::WHITE,
                    );
                }
            });
        });

        if matches!(self.current.stage, RenderStage::DisplayingImage(_)) {
            egui::TopBottomPanel::new(TopBottomSide::Bottom, "bottom-panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut needs_to_reset = false;
                    if let RenderStage::DisplayingImage(index) = &mut self.current.stage {
                        ui.label("History: ");
                        let previous = *index;

                        #[allow(clippy::range_minus_one)] //😔
                        ui.add(Slider::new(
                            index,
                            0..=(self.current.image_history.len() - 1),
                        ));

                        let mut needs_to_update_settings = previous != *index;

                        if ui.button("Clear History").clicked() {
                            self.current.image_history.clear();
                            needs_to_reset = true;
                        }

                        if ui.button("Remove Current Image").clicked() {
                            if self.current.image_history.len() == 1 {
                                self.current.image_history.clear();
                                needs_to_reset = true;
                            } else {
                                self.current.image_history.remove(*index);

                                if *index == self.current.image_history.len() {
                                    *index = self.current.image_history.len() - 1;
                                }

                                needs_to_update_settings = true;
                            }
                        }

                        if ui.button("Set History to Current").clicked() {
                            let current = self.current.image_history.swap_remove(*index);
                            self.current.image_history = vec![current];
                            *index = 0;
                            needs_to_update_settings = true;
                        }

                        if needs_to_update_settings {
                            let (palette, output, distance) =
                                self.current.image_history[*index].settings;
                            self.palette_settings = palette;
                            self.output_settings = output;
                            self.distance_algorithm = distance;

                            self.needs_to_refresh_output = false;
                            self.needs_to_refresh_palette = false;
                        }
                    }

                    ui.separator();

                    if let RenderStage::DisplayingImage(index) = &self.current.stage {
                        if ui.button("Save").clicked() {
                            self.current.save_file(*index);
                        }
                    }

                    if needs_to_reset {
                        self.current.stage = RenderStage::Nothing;
                        self.distance_algorithm = DistanceAlgorithm::Euclidean;
                        self.palette_settings = PaletteSettings::default();
                        self.output_settings = OutputSettings::default();
                        self.needs_to_refresh_output = false;
                        self.needs_to_refresh_palette = false;
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
                RenderStage::CreatingPalette { last_progress, .. } => {
                    ui.label("Creating palette...");

                    let (so_far, max) = last_progress;
                    ProgressBar::new((*so_far as f32) / (*max as f32))
                        .animate(true)
                        .show_percentage()
                        .ui(ui);
                }
                RenderStage::CreatingOutput { last_progress, .. } => {
                    ui.label("Converting and dithering...");

                    let (so_far, max) = last_progress;
                    ProgressBar::new((*so_far as f32) / (*max as f32))
                        .animate(true)
                        .show_percentage()
                        .ui(ui);
                }
                RenderStage::DisplayingImage(index) => {
                    let RenderedImage { output, handle, .. } = &self.current.image_history[*index];

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

    fn save(&mut self, storage: &mut dyn Storage) {
        if self.current.last_start_save_dirs.0.is_some()
            || self.current.last_start_save_dirs.1.is_some()
        {
            if let Ok(sered) = serde_json::to_string(&self.current.last_start_save_dirs) {
                storage.set_string("start_and_save_dirs", sered);
            }
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
