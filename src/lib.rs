use image::{ColorType, DynamicImage, GenericImage, GenericImageView, Pixel, Rgba};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DistanceAlgorithm {
    Euclidean,
    Manhattan,
}

impl DistanceAlgorithm {
    pub const fn to_str(self) -> &'static str {
        match self {
            Self::Euclidean => "Euclidean",
            Self::Manhattan => "Manhattan",
        }
    }
}

impl Display for DistanceAlgorithm {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

pub const ALL_ALGOS: [DistanceAlgorithm; 2] =
    [DistanceAlgorithm::Euclidean, DistanceAlgorithm::Manhattan];

impl DistanceAlgorithm {
    pub const fn distance(self, a: Rgba<u8>, b: Rgba<u8>) -> u32 {
        #[inline]
        const fn euclidean_distance(
            Rgba([r, g, b, _]): Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(cmp_r);
            let delta_g = g.abs_diff(cmp_g);
            let delta_b = b.abs_diff(cmp_b);

            (delta_r as u32).pow(2) + (delta_g as u32).pow(2) + (delta_b as u32).pow(2)
        }

        #[inline]
        const fn manhattan_distance(
            Rgba([r, g, b, _]): Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(cmp_r);
            let delta_g = g.abs_diff(cmp_g);
            let delta_b = b.abs_diff(cmp_b);

            delta_r as u32 + delta_g as u32 + delta_b as u32
        }

        match self {
            Self::Euclidean => euclidean_distance(a, b),
            Self::Manhattan => manhattan_distance(a, b),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct PaletteSettings {
    pub chunks_per_dimension: u32,
    pub closeness_threshold: u32,
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
pub struct OutputSettings {
    pub output_px_size: u32,
    pub dithering_likelihood: u32,
    pub dithering_scale: u32,
    pub scale_output_to_original: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            output_px_size: 32,
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
    let closeness_threshold =
        get_closest_factor(closeness_threshold, image.width().min(image.height()));

    let (width, height) = image.dimensions();
    let chunks_per_dimension = chunks_per_dimension.min(width).min(height);
    let (width_chunk_size, height_chunk_size) =
        (width / chunks_per_dimension, height / chunks_per_dimension);

    let max_num_colours = chunks_per_dimension * chunks_per_dimension;
    let mut progress_bar = 0;
    let mut av_px_colours = Vec::with_capacity(max_num_colours as usize);

    for chunk_x in 0..chunks_per_dimension {
        for chunk_y in 0..chunks_per_dimension {
            if stop.load(Ordering::Relaxed) {
                return av_px_colours;
            }

            let mut map: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y);

                    let mut too_close = false;
                    for so_far in av_px_colours.iter().copied() {
                        if dist_algo.distance(px, so_far) < closeness_threshold {
                            too_close = true;
                            break;
                        }
                    }

                    if !too_close {
                        *map.entry(px).or_default() += 1;
                    }
                }
            }

            if let Some((most_common, _)) = map.into_iter().max_by_key(|(_, count)| *count) {
                av_px_colours.push(most_common);
            }

            progress_bar += 1;
            let _ = progress_sender.send((progress_bar, max_num_colours));
        }
    }

    av_px_colours
}

pub fn dither_palette(
    input: &DynamicImage,
    palette: &[Rgba<u8>],
    distance_algorithm: DistanceAlgorithm,
    OutputSettings {
        output_px_size,
        dithering_likelihood,
        dithering_scale,
        scale_output_to_original: output_img_scaling,
    }: OutputSettings,
    progress_sender: &Sender<(u32, u32)>,
    stop: Arc<AtomicBool>,
) -> DynamicImage {
    let output_px_size = get_closest_factor(output_px_size, input.height());

    let (width, height) = input.dimensions();

    let (num_width_chunks, num_height_chunks) = (width / output_px_size, height / output_px_size);
    let (output_w, output_h) = if dithering_likelihood == 1 {
        (num_width_chunks, num_height_chunks)
    } else {
        (
            num_width_chunks * dithering_scale,
            num_height_chunks * dithering_scale,
        )
    };
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

            let distances: HashMap<_, _> = palette
                .iter()
                .copied()
                .map(|x| (x, distance_algorithm.distance(x, av_px)))
                .collect();

            let mut cloned_palette = palette.to_vec();
            cloned_palette.sort_by_key(|rgb| distances[rgb]);

            let (first, second) = if cloned_palette.len() == 1 {
                let first = cloned_palette.remove(0);
                (first, first)
            } else {
                let second = cloned_palette.swap_remove(1).to_rgba();
                let first = cloned_palette.swap_remove(0).to_rgba();

                let first_distance = distances[&first];
                let second_distance = distances[&second];

                let inter_candidate_distance = distance_algorithm.distance(first, second);

                if first_distance.abs_diff(second_distance)
                    > (inter_candidate_distance / dithering_likelihood)
                {
                    (first, first)
                } else {
                    (first, second)
                }
            };

            if dithering_scale == 1 || dithering_likelihood == 1 {
                for px_x in (dithering_scale * chunk_x)..(dithering_scale * (chunk_x + 1)) {
                    for px_y in (dithering_scale * chunk_y)..(dithering_scale * (chunk_y + 1)) {
                        output.put_pixel(px_x, px_y, first);
                    }
                }
            }

            for px_x in (dithering_scale * chunk_x)..(dithering_scale * (chunk_x + 1)) {
                for px_y in (dithering_scale * chunk_y)..(dithering_scale * (chunk_y + 1)) {
                    let mut should_dither = px_y % 2 == 0;
                    if px_x % 2 == 0 {
                        should_dither = !should_dither;
                    }

                    output.put_pixel(px_x, px_y, if should_dither { first } else { second });
                }
            }

            chunks_progress_bar += 1;
            let _ = progress_sender.send((chunks_progress_bar, total_chunks));
        }
    }

    if !output_img_scaling {
        return output;
    }

    //yes this is the lazy way of doing things
    //compared to doing it in the above loop
    //but
    //this logic is vastly simpler
    //and it's not like it takes that long
    let scaling_factor = if dithering_likelihood == 1 || dithering_scale == 1 {
        output_px_size
    } else {
        output_px_size / dithering_scale
    };

    let (final_w, final_h) = (output_w * scaling_factor, output_h * scaling_factor);
    let mut final_img = DynamicImage::new(final_w, final_h, ColorType::Rgb8);

    for x in 0..output_w {
        for y in 0..output_h {
            let px = output.get_pixel(x, y);

            for px_x in (scaling_factor * x)..(scaling_factor * (x + 1)) {
                for px_y in (scaling_factor * y)..(scaling_factor * (y + 1)) {
                    final_img.put_pixel(px_x, px_y, px);
                }
            }
        }
    }

    final_img
}
