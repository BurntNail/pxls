use image::{ColorType, DynamicImage, GenericImage, GenericImageView, Pixel, Rgba};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use crate::pixel_operations::{average, better_luminance, hue, luminance, product};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DistanceAlgorithm {
    Euclidean,
    Manhattan,
    Product,
    Brightness,
    Luminance,
    SlowLuminance,
    Hue,
}

impl DistanceAlgorithm {
    pub const fn to_str(self) -> &'static str {
        match self {
            Self::Euclidean => "Euclidean",
            Self::Manhattan => "Manhattan",
            Self::Product => "Product",
            Self::Brightness => "Brightness",
            Self::Luminance => "Luminance",
            Self::SlowLuminance => "SlowLuminance",
            Self::Hue => "Hue",
        }
    }

    pub const fn standardise_closeness_threshold(self, n: u32) -> u32 {
        match self {
            Self::Euclidean | Self::Product => n * n,
            Self::Manhattan | Self::Brightness | Self::Luminance | Self::SlowLuminance | Self::Hue => n,
        }
    }
}

impl Display for DistanceAlgorithm {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

pub const ALL_ALGOS: &[DistanceAlgorithm] = &[
    DistanceAlgorithm::Euclidean,
    DistanceAlgorithm::Manhattan,
    DistanceAlgorithm::Product,
    DistanceAlgorithm::Brightness,
    DistanceAlgorithm::Luminance,
    DistanceAlgorithm::SlowLuminance,
    DistanceAlgorithm::Hue
];

pub mod pixel_operations {
    use image::Rgba;

    #[inline]
    pub const fn product(
        Rgba([r, g, b, _]): Rgba<u8>,
    ) -> u32 {
        r as u32 * g as u32 * b as u32
    }

    // https://stackoverflow.com/questions/596216/formula-to-determine-perceived-brightness-of-rgb-color :)
    #[inline]
    pub const fn luminance(
        Rgba([r, g, b, _]): Rgba<u8>,
    ) -> u32 {
        ((r as u32) * 1063 / 5000) + ((g as u32) * 447 / 625) + ((b as u32) * 361 / 5000)
    }

    #[inline]
    pub const fn better_luminance(
        Rgba([r, g, b, _]): Rgba<u8>,
    ) -> u32 {
        ((r as u32).pow(2) * 299 / 1000)
            + ((g as u32).pow(2) * 587 / 1000)
            + ((b as u32).pow(2) * 57 / 500)
    }

    #[inline]
    pub const fn average(
        Rgba([r, g, b, _]): Rgba<u8>,
    ) -> u32 {
        (r as u32 + g as u32 + b as u32) / 3
    }

    //https://stackoverflow.com/questions/23090019/fastest-formula-to-get-hue-from-rgb
    pub fn hue (
        Rgba([r, g, b, _]): Rgba<u8>
    ) -> u32 {
        let min = r.min(g).min(b);
        let max = r.max(g).max(b);

        let delta = max - min;

        let (rf, gf, bf, deltaf) = (r as f32, g as f32, b as f32, delta as f32);

        if min == max {
            return 0;
        }

        let mut hue = if max == r {
            (gf - bf) / deltaf
        } else if max == g {
            2.0 + (bf - rf) / deltaf
        } else {
            4.0 + (rf - gf) / deltaf
        };

        hue *= 60.0;
        if hue < 0.0 {
            hue += 360.0;
        }

        hue.round() as u32
    }
}

impl DistanceAlgorithm {
    pub fn distance(self, a: Rgba<u8>, b: Rgba<u8>) -> u32 {
        #[inline]
        pub const fn euclidean_distance(
            Rgba([r, g, b, _]): Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(cmp_r) as u32;
            let delta_g = g.abs_diff(cmp_g) as u32;
            let delta_b = b.abs_diff(cmp_b) as u32;

            delta_r.pow(2) + delta_g.pow(2) + delta_b.pow(2)
        }

        #[inline]
        const fn manhattan_distance(
            Rgba([r, g, b, _]): Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(cmp_r) as u32;
            let delta_g = g.abs_diff(cmp_g) as u32;
            let delta_b = b.abs_diff(cmp_b) as u32;

            delta_r + delta_g + delta_b
        }


        match self {
            Self::Euclidean => euclidean_distance(a, b),
            Self::Manhattan => manhattan_distance(a, b),
            Self::Product => product(a).abs_diff(product(b)),
            Self::Brightness => average(a).abs_diff(average(b)),
            Self::Luminance => luminance(a).abs_diff(luminance(b)),
            Self::SlowLuminance => better_luminance(a).abs_diff(better_luminance(b)),
            Self::Hue => hue(a).abs_diff(hue(b))
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PaletteSettings {
    pub chunks_per_dimension: u32,
    pub closeness_threshold: u32,
}

impl Default for PaletteSettings {
    fn default() -> Self {
        Self {
            chunks_per_dimension: 100,
            closeness_threshold: 50,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct OutputSettings {
    pub output_px_size: u32,
    pub dithering_likelihood: u32,
    pub dithering_scale: u32,
    pub scale_output_to_original: bool,
}

impl PartialEq for OutputSettings {
    fn eq(&self, other: &Self) -> bool {
        if self.dithering_scale == 1 || other.dithering_scale == 1 {
            if self.dithering_scale != other.dithering_scale {
                false
            } else {
                self.output_px_size == other.output_px_size
                    && self.scale_output_to_original == other.scale_output_to_original
            }
        } else {
            self.output_px_size == other.output_px_size
                && self.dithering_likelihood == other.dithering_likelihood
                && self.dithering_scale == other.dithering_scale
                && self.scale_output_to_original == other.scale_output_to_original
        }
    }
}

impl Eq for OutputSettings {}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            output_px_size: 5,
            dithering_likelihood: 4,
            dithering_scale: 2,
            scale_output_to_original: true,
        }
    }
}

//tyvm https://stackoverflow.com/questions/26885198/find-closest-factor-to-a-number-of-a-number
pub fn get_closest_factor(target: u32, number: u32) -> u32 {
    for i in 0..number {
        if number % (target + i) == 0 {
            return target + i;
        } else if number % (target - i) == 0 {
            return target - i;
        }
    }
    number
}

pub fn get_palette(
    image: &DynamicImage,
    PaletteSettings {
        chunks_per_dimension,
        closeness_threshold,
    }: PaletteSettings,
    dist_algo: DistanceAlgorithm,
    progress_sender: &Sender<(u32, u32)>,
    stop: Arc<AtomicBool>,
) -> Vec<Rgba<u8>> {
    let chunks_per_dimension =
        get_closest_factor(chunks_per_dimension, image.width().min(image.height()));
    let (width_chunk_size, height_chunk_size) = (
        image.width() / chunks_per_dimension,
        image.height() / chunks_per_dimension,
    );

    let num_chunks = chunks_per_dimension * chunks_per_dimension;
    let mut progress_bar = 0;

    let mut av_px_colours = Vec::with_capacity(num_chunks as usize);
    let mut cache = HashMap::new();

    for chunk_x in 0..chunks_per_dimension {
        for chunk_y in 0..chunks_per_dimension {
            if stop.load(Ordering::Relaxed) {
                return av_px_colours;
            }

            let mut occurencces_of_suitably_far: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y);

                    let too_close = match cache.entry(px) {
                        Entry::Occupied(occ) => *occ.get(),
                        Entry::Vacant(vac) => {
                            let mut too_close = false;
                            for so_far in av_px_colours.iter().copied() {
                                if dist_algo.distance(px, so_far)
                                    < dist_algo.standardise_closeness_threshold(closeness_threshold)
                                {
                                    too_close = true;
                                    break;
                                }
                            }

                            *vac.insert(too_close)
                        }
                    };

                    if !too_close {
                        *occurencces_of_suitably_far.entry(px).or_default() += 1;
                    }
                }
            }

            if let Some((most_common, _)) = occurencces_of_suitably_far
                .into_iter()
                .max_by_key(|(_, count)| *count)
            {
                av_px_colours.push(most_common);
                cache.clear();
            }

            progress_bar += 1;
            let _ = progress_sender.send((progress_bar, num_chunks));
        }
    }

    av_px_colours
}

pub fn dither_original_with_palette(
    input: &DynamicImage,
    palette: &[Rgba<u8>],
    distance_algorithm: DistanceAlgorithm,
    output_settings: OutputSettings,
    progress_sender: &Sender<(u32, u32)>,
    stop: Arc<AtomicBool>,
) -> DynamicImage {
    let output_px_size =
        get_closest_factor(1 << (output_settings.output_px_size - 1), input.width());

    let (width, height) = input.dimensions();

    let (num_width_chunks, num_height_chunks) = (width / output_px_size, height / output_px_size);
    let (output_w, output_h) = (
        num_width_chunks * output_settings.dithering_scale,
        num_height_chunks * output_settings.dithering_scale,
    );

    let mut output = DynamicImage::new(output_w, output_h, ColorType::Rgb8);

    let total_chunks = num_width_chunks * num_height_chunks;
    let mut chunks_progress_bar = 0;

    for chunk_x in 0..num_width_chunks {
        for chunk_y in 0..num_height_chunks {
            if stop.load(Ordering::Relaxed) {
                return output;
            }

            let (mut accum_r, mut accum_g, mut accum_b) = (0_u64, 0_u64, 0_u64);

            for px_x in (output_px_size * chunk_x)..(output_px_size * (chunk_x + 1)) {
                for px_y in (output_px_size * chunk_y)..(output_px_size * (chunk_y + 1)) {
                    let [r, g, b] = input.get_pixel(px_x, px_y).to_rgb().0;
                    accum_r += r as u64;
                    accum_g += g as u64;
                    accum_b += b as u64;
                }
            }

            let divisor = (output_px_size * output_px_size) as u64;

            let av_px = Rgba([
                (accum_r / divisor) as u8,
                (accum_g / divisor) as u8,
                (accum_b / divisor) as u8,
                u8::MAX,
            ]);

            let mut first = None;
            let mut first_distance = u32::MAX;
            let mut second = None;
            let mut second_distance = u32::MAX;

            for px in palette.iter().copied() {
                let dist = distance_algorithm.distance(px, av_px);

                if dist < first_distance {
                    second = first;
                    second_distance = first_distance;

                    first = Some(px);
                    first_distance = dist;
                } else if dist < second_distance {
                    second = Some(px);
                    second_distance = dist;
                }
            }

            let first = first.unwrap();
            let mut second = second.unwrap_or(first);

            //TODO: make DL more ergonomic and easier to understand
            if first_distance.abs_diff(second_distance)
                > (distance_algorithm.distance(first, second)
                    / output_settings.dithering_likelihood)
            {
                second = first;
            }

            for px_x in (output_settings.dithering_scale * chunk_x)
                ..(output_settings.dithering_scale * (chunk_x + 1))
            {
                for px_y in (output_settings.dithering_scale * chunk_y)
                    ..(output_settings.dithering_scale * (chunk_y + 1))
                {
                    let mut is_even_px = px_y % 2 == 0;
                    if px_x % 2 == 0 {
                        is_even_px = !is_even_px;
                    }
                    is_even_px &= output_settings.dithering_scale > 1;

                    output.put_pixel(px_x, px_y, if is_even_px { first } else { second });
                }
            }

            chunks_progress_bar += 1;
            let _ = progress_sender.send((chunks_progress_bar, total_chunks));
        }
    }

    pixel_perfect_scale(output_settings, &output)
}

pub fn pixel_perfect_scale(output_settings: OutputSettings, from: &DynamicImage) -> DynamicImage {
    if !output_settings.scale_output_to_original {
        return from.clone();
    }

    let scaling_factor =
        (1 << (output_settings.output_px_size - 1)) / output_settings.dithering_scale;

    let (final_w, final_h) = (
        from.width() * scaling_factor,
        from.height() * scaling_factor,
    );
    let mut final_img = DynamicImage::new(final_w, final_h, ColorType::Rgb8);

    for x in 0..from.width() {
        for y in 0..from.height() {
            let px = from.get_pixel(x, y);

            for px_x in (scaling_factor * x)..(scaling_factor * (x + 1)) {
                for px_y in (scaling_factor * y)..(scaling_factor * (y + 1)) {
                    final_img.put_pixel(px_x, px_y, px);
                }
            }
        }
    }

    final_img
}
