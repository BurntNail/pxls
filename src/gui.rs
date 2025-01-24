use crate::logic::{dither_palette, get_palette, DistanceAlgorithm, ALL_ALGOS};
use eframe::{CreationContext, Frame, NativeOptions};
use egui::panel::TopBottomSide;
use egui::{pos2, Color32, ColorImage, Context, Rect, TextureHandle, TextureId, TextureOptions};
use egui_extras::install_image_loaders;
use image::{DynamicImage, GenericImageView, ImageReader, Pixel, Rgba};
use rfd::FileDialog;
use std::env::current_dir;
use std::path::PathBuf;

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

struct PhotoBeingEdited {
    input: DynamicImage,
    palette: Vec<Rgba<u8>>,
    handle: TextureHandle,
    texture_options: TextureOptions,
    output: DynamicImage,
    output_file_name: String,
}

impl PhotoBeingEdited {
    pub fn new(
        input_file: PathBuf,
        palette_settings: PaletteSettings,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        ctx: &Context,
    ) -> anyhow::Result<Self> {
        let input = ImageReader::open(input_file)?.decode()?;
        let palette = get_palette(
            &input,
            palette_settings.chunks_per_dimension,
            palette_settings.closeness_threshold,
            distance_algorithm,
        );
        let output = dither_palette(
            &input,
            &palette,
            distance_algorithm,
            output_settings.output_px_size,
            output_settings.dithering_factor,
        );

        let texture_options = TextureOptions::LINEAR;

        let handle = ctx.load_texture(
            "img",
            Self::color_image_from_dynamic_image(&output),
            texture_options,
        );

        Ok(Self {
            input,
            palette,
            handle,
            texture_options,
            output,
            output_file_name: "output.jpeg".to_string(),
        })
    }

    pub fn change_palette_settings_or_algo(
        &mut self,
        palette_settings: PaletteSettings,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        ctx: &Context,
    ) {
        let palette = get_palette(
            &self.input,
            palette_settings.chunks_per_dimension,
            palette_settings.closeness_threshold,
            distance_algorithm,
        );
        let output = dither_palette(
            &self.input,
            &palette,
            distance_algorithm,
            output_settings.output_px_size,
            output_settings.dithering_factor,
        );
        let handle = ctx.load_texture(
            "my-img",
            Self::color_image_from_dynamic_image(&output),
            self.texture_options,
        );

        self.palette = palette;
        self.output = output;
        self.handle = handle;
    }

    pub fn change_output_settings(
        &mut self,
        output_settings: OutputSettings,
        distance_algorithm: DistanceAlgorithm,
        ctx: &Context,
    ) {
        let output = dither_palette(
            &self.input,
            &self.palette,
            distance_algorithm,
            output_settings.output_px_size,
            output_settings.dithering_factor,
        );
        let handle = ctx.load_texture(
            "my-img",
            Self::color_image_from_dynamic_image(&output),
            self.texture_options,
        );

        self.output = output;
        self.handle = handle;
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

#[derive(Copy, Clone, Debug)]
struct PaletteSettings {
    chunks_per_dimension: u32,
    closeness_threshold: u32,
}

impl Default for PaletteSettings {
    fn default() -> Self {
        Self {
            chunks_per_dimension: 100,
            closeness_threshold: 2_500,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct OutputSettings {
    output_px_size: u32,
    dithering_factor: u32,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            output_px_size: 32,
            dithering_factor: 4,
        }
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
    current: Option<PhotoBeingEdited>,
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
            current: None,
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
        egui::TopBottomPanel::new(TopBottomSide::Top, "top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    if ui.button("Select File").clicked() {
                        if let Some(file) = FileDialog::new()
                            .add_filter("images", &["jpg", "png", "jpeg"])
                            .set_directory(current_dir().unwrap_or_else(|_| "/".into()))
                            .pick_file()
                        {
                            match PhotoBeingEdited::new(
                                file,
                                self.palette_settings,
                                self.output_settings,
                                self.distance_algorithm,
                                ctx,
                            ) {
                                Ok(wked) => self.current = Some(wked),
                                Err(e) => {
                                    eprintln!("Error creating new photo being edited: {e:?}");
                                }
                            }
                        }
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
                    if let Some(current) = self.current.as_mut() {
                        let mut needs_to_update = self.auto_update;
                        if !needs_to_update {
                            ui.separator();
                            needs_to_update = ui.button("Update").clicked();
                        }

                        if needs_to_update {
                            if self.needs_to_refresh_palette {
                                current.change_palette_settings_or_algo(
                                    self.palette_settings,
                                    self.output_settings,
                                    self.distance_algorithm,
                                    ctx,
                                );
                            } else if self.needs_to_refresh_output {
                                current.change_output_settings(
                                    self.output_settings,
                                    self.distance_algorithm,
                                    ctx,
                                );
                            }

                            self.needs_to_refresh_palette = false;
                            self.needs_to_refresh_output = false;
                        }
                    }
                }
            });
        });

        if let Some(current) = self.current.as_mut() {
            egui::CentralPanel::default().show(ctx, |ui| {
                let texture_id = TextureId::from(&current.handle);
                let available_width = ui.available_width();
                let available_height = ui.available_height();
                let available_aspect = available_width / available_height;

                let (img_width, img_height) =
                    (current.input.width() as f32, current.input.height() as f32);
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
            });

            egui::TopBottomPanel::new(TopBottomSide::Bottom, "bottom-panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Output File Name: ");
                    ui.text_edit_singleline(&mut current.output_file_name);

                    ui.separator();

                    if ui.button("Save").clicked() {
                        if let Err(e) = current.output.save(&current.output_file_name) {
                            eprintln!("Error saving file: {e:?}");
                        }
                    }
                })
            });
        }
    }
}
