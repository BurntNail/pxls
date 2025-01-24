use std::env::current_dir;
use std::path::PathBuf;
use eframe::{CreationContext, Frame, NativeOptions};
use egui::{pos2, Color32, ColorImage, Context, Rect, TextureHandle, TextureId, TextureOptions};
use egui::panel::TopBottomSide;
use egui_extras::install_image_loaders;
use image::{DynamicImage, GenericImageView, ImageReader, Pixel, Rgba};
use rfd::FileDialog;
use crate::logic::{dither_palette, get_palette, DistanceAlgorithm, ALL_ALGOS};

pub fn gui_main () {
    let native_options = NativeOptions::default();

    if let Err(e) = eframe::run_native("Pxls", native_options, Box::new(|cc| Ok(Box::new(PxlsApp::new(cc))))) {
        eprintln!("Error running eframe: {e:?}");
    }
}

struct PhotoBeingEdited {
    input: DynamicImage,
    palette: Vec<Rgba<u8>>,
    handle: TextureHandle,
    texture_options: TextureOptions
}

impl PhotoBeingEdited {
    pub fn new (input_file: PathBuf, palette_settings: PaletteSettings, output_settings: OutputSettings, distance_algorithm: DistanceAlgorithm, ctx: &Context) -> anyhow::Result<Self> {
        let input = ImageReader::open(input_file)?.decode()?;
        let palette = get_palette(&input, palette_settings.chunks_per_dimension, palette_settings.closeness_threshold, distance_algorithm);
        let output = dither_palette(&input, &palette, distance_algorithm, output_settings.output_px_size);

        let texture_options = TextureOptions::LINEAR;

        let handle = ctx.load_texture("my-img", Self::color_image_from_dynamic_image(output), texture_options);

        Ok(Self {
            input,
            palette,
            handle,
            texture_options
        })
    }

    pub fn change_palette_settings_or_algo(&mut self, palette_settings: PaletteSettings, output_settings: OutputSettings, distance_algorithm: DistanceAlgorithm, ctx: &Context) {
        let palette = get_palette(&self.input, palette_settings.chunks_per_dimension, palette_settings.closeness_threshold, distance_algorithm);
        let output = dither_palette(&self.input, &palette, distance_algorithm, output_settings.output_px_size);
        let handle = ctx.load_texture("my-img", Self::color_image_from_dynamic_image(output), self.texture_options);

        self.palette = palette;
        self.handle = handle;
    }

    pub fn change_output_settings (&mut self, output_settings: OutputSettings, distance_algorithm: DistanceAlgorithm, ctx: &Context) {
        let output = dither_palette(&self.input, &self.palette, distance_algorithm, output_settings.output_px_size);
        let handle = ctx.load_texture("my-img", Self::color_image_from_dynamic_image(output), self.texture_options);

        self.handle = handle;
    }

    fn color_image_from_dynamic_image (img: DynamicImage) -> ColorImage {
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
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            output_px_size: 32,
        }
    }
}


struct PxlsApp {
    current: Option<PhotoBeingEdited>,
    distance_algorithm: DistanceAlgorithm,
    palette_settings: PaletteSettings,
    output_settings: OutputSettings
}

impl PxlsApp {
    pub fn new (cc: &CreationContext<'_>) -> Self {
        install_image_loaders(&cc.egui_ctx);
        Self {
            current: None,
            distance_algorithm: DistanceAlgorithm::Euclidean,
            palette_settings: PaletteSettings::default(),
            output_settings: OutputSettings::default()
        }
    }
}

impl eframe::App for PxlsApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::TopBottomPanel::new(TopBottomSide::Top, "top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Select File").clicked() {
                    if let Some(file) = FileDialog::new()
                        .add_filter("images", &["jpg", "png", "jpeg"])
                        .set_directory(current_dir().unwrap_or_else(|_| "/".into()))
                        .pick_file() {
                        match PhotoBeingEdited::new(file, self.palette_settings, self.output_settings, self.distance_algorithm, &ctx) {
                            Ok(wked) => self.current = Some(wked),
                            Err(e) => {
                                eprintln!("Error creating new photo being edited: {e:?}");
                            }
                        }
                    }
                }

                ui.vertical(|ui| {
                    let current = self.distance_algorithm;
                    for possibility in ALL_ALGOS {
                        ui.radio_value(&mut self.distance_algorithm, possibility, possibility.to_str());
                    }

                    if current != self.distance_algorithm {
                        if let Some(current) = self.current.as_mut() {
                            current.change_palette_settings_or_algo(self.palette_settings, self.output_settings, self.distance_algorithm, ctx);
                        }
                    }
                });
            });
        });

        if let Some(current) = self.current.as_mut() {
            egui::CentralPanel::default().show(ctx, |ui| {
                let texture_id = TextureId::from(&current.handle);
                let available_width = ui.available_width();
                let available_height = ui.available_height();
                let available_aspect = available_width / available_height;

                let (img_width, img_height) = (
                    current.input.width() as f32,
                    current.input.height() as f32
                );
                let img_aspect = img_width / img_height;

                let (uv_x, uv_y) = if available_aspect > img_aspect {
                    (available_aspect / img_aspect, 1.0)
                } else {
                    (1.0, img_aspect / available_aspect)
                };


                let uv = Rect{ min:pos2(0.0, 0.0), max:pos2(uv_x, uv_y)};

                ui.painter().image(texture_id, ui.available_rect_before_wrap(), uv, Color32::WHITE);
            });
        }
    }
}