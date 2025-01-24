use std::collections::HashMap;
use image::{ColorType, DynamicImage, GenericImage, GenericImageView, Pixel, Rgba};
use indicatif::ProgressBar;

#[derive(Debug, Copy, Clone)]
pub enum DistanceAlgorithm {
    Euclidean,
    Manhattan,
}

impl DistanceAlgorithm {
    pub fn distance(&self, a: &Rgba<u8>, b: &Rgba<u8>) -> u32 {
        #[inline]
        fn euclidean_distance(
            Rgba([r, g, b, _]): &Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): &Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(*cmp_r);
            let delta_g = g.abs_diff(*cmp_g);
            let delta_b = b.abs_diff(*cmp_b);

            (delta_r as u32).pow(2) + (delta_g as u32).pow(2) + (delta_b as u32).pow(2)
        }

        #[inline]
        fn manhattan_distance(
            Rgba([r, g, b, _]): &Rgba<u8>,
            Rgba([cmp_r, cmp_g, cmp_b, _]): &Rgba<u8>,
        ) -> u32 {
            let delta_r = r.abs_diff(*cmp_r);
            let delta_g = g.abs_diff(*cmp_g);
            let delta_b = b.abs_diff(*cmp_b);

            delta_r as u32 + delta_g as u32 + delta_b as u32
        }

        match self {
            Self::Euclidean => euclidean_distance(a, b),
            Self::Manhattan => manhattan_distance(a, b),
        }
    }
}

pub fn get_palette(
    image: &DynamicImage,
    chunks_per_dimension: u32,
    closeness_threshold: u32,
    dist_algo: DistanceAlgorithm,
) -> Vec<Rgba<u8>> {
    let (width, height) = image.dimensions();
    let chunks_per_dimension = chunks_per_dimension.min(width).min(height);
    let (width_chunk_size, height_chunk_size) =
        (width / chunks_per_dimension, height / chunks_per_dimension);

    let max_num_colours = chunks_per_dimension * chunks_per_dimension;
    let progress_bar = ProgressBar::new(max_num_colours as u64);
    let mut av_px_colours = Vec::with_capacity(max_num_colours as usize);
    for chunk_x in 0..chunks_per_dimension {
        for chunk_y in 0..chunks_per_dimension {
            let mut map: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y);

                    let mut too_close = false;
                    for so_far in &av_px_colours {
                        if dist_algo.distance(&px, so_far) < closeness_threshold {
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

            progress_bar.inc(1);
        }
    }

    av_px_colours
}


pub fn dither_palette(
    input: &DynamicImage,
    palette: &[Rgba<u8>],
    distance_algorithm: DistanceAlgorithm,
    output_px_size: u32,
) -> DynamicImage {
    let (width, height) = input.dimensions();

    let (num_width_chunks, num_height_chunks) = (width / output_px_size, height / output_px_size);
    let mut output = DynamicImage::new(width, height, ColorType::Rgb8);

    let chunks_progress_bar = ProgressBar::new((num_width_chunks * num_height_chunks) as u64);

    for chunk_x in 0..num_width_chunks {
        for chunk_y in 0..num_height_chunks {
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
                u8::MAX
            ]);

            let distances: HashMap<_, _> = palette.iter().cloned().map(|x| (x, distance_algorithm.distance(&x, &av_px))).collect();

            let mut cloned_palette = palette.to_vec();
            cloned_palette.sort_by_key(|rgb| distances[rgb]);

            let mut second = cloned_palette.swap_remove(1).to_rgba();
            let first = cloned_palette.swap_remove(0).to_rgba();

            let first_distance = distances[&first];
            let second_distance = distances[&second];

            let inter_candidate_distance = distance_algorithm.distance(&first, &second);

            if first_distance.abs_diff(second_distance) > (inter_candidate_distance / 4) {
                second = first;
            }

            for px_x in (output_px_size * chunk_x)..(output_px_size * (chunk_x + 1)) {
                for px_y in (output_px_size * chunk_y)..(output_px_size * (chunk_y + 1)) {
                    if output_px_size == 1 {
                        output.put_pixel(px_x, px_y, first);
                    } else {
                        let mut should_dither = (px_y % (output_px_size / 2)) < (output_px_size / 4);

                        if (px_x % (output_px_size / 2)) < (output_px_size / 4) {
                            should_dither = !should_dither;
                        }

                        if should_dither {
                            output.put_pixel(px_x, px_y, first);
                        }  else {
                            output.put_pixel(px_x, px_y, second);
                        }
                    }
                }
            }

            chunks_progress_bar.inc(1);
        }
    }

    output
}
