use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use dialoguer::{FuzzySelect, Input};
use dialoguer::theme::ColorfulTheme;
use image::{ColorType, DynamicImage, GenericImage, GenericImageView, ImageReader, Pixel, Rgb};
use indicatif::{MultiProgress, ProgressBar};

struct Args {
    input: PathBuf,
    output: PathBuf,
    chunks_per_dimension: u32,
    closeness_threshold: u32,
    output_px_size: u32,
    algorithm: DistanceAlgorithm
}

#[derive(Debug, Copy, Clone)]
pub enum DistanceAlgorithm {
    Euclidean,
    Manhattan
}

impl DistanceAlgorithm {
    fn distance (&self, a: &Rgb<u8>, b: &Rgb<u8>) -> u32 {
        #[inline]
        fn euclidean_distance(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
            let delta_r = r.abs_diff(*cmp_r);
            let delta_g = g.abs_diff(*cmp_g);
            let delta_b = b.abs_diff(*cmp_b);

            (delta_r as u32).pow(2) + (delta_g as u32).pow(2) + (delta_b as u32).pow(2)
        }

        #[inline]
        fn manhattan_distance(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
            let delta_r = r.abs_diff(*cmp_r);
            let delta_g = g.abs_diff(*cmp_g);
            let delta_b = b.abs_diff(*cmp_b);

            delta_r as u32 + delta_g as u32 + delta_b as u32
        }

        match self {
            Self::Euclidean => euclidean_distance(a, b),
            Self::Manhattan => manhattan_distance(a, b)
        }
    }
}

impl Args {
    pub fn parse () -> anyhow::Result<Self> {
        match Self::parse_env() {
            Some(x) => Ok(x),
            None => Self::parse_manual()
        }
    }

    fn parse_env () -> Option<Self> {
        None //TODO: do this
    }

    fn parse_manual () -> anyhow::Result<Self> {
        let theme = ColorfulTheme::default();
        let input = {
            let mut current_dir_fns = vec![];
            let mut current_dir_files = vec![];

            for entry in fs::read_dir(".")? {
                let entry = entry?;
                if entry.metadata()?.is_file() {
                    let path = entry.path();
                    current_dir_fns.push(format!("{path:?}"));
                    current_dir_files.push(path);
                }
            }

            let chosen = FuzzySelect::with_theme(&theme)
                .with_prompt("Which file?")
                .items(&current_dir_fns)
                .interact()?;

            current_dir_files.swap_remove(chosen)
        };
        let chunks_per_dimension = Input::with_theme(&theme).with_prompt("How many chunks per dimension should be used for palette generation?").interact()?;
        let closeness_threshold = Input::with_theme(&theme).with_prompt("What should the closeness threshold be for palette generation?").interact()?;
        let algorithm = if FuzzySelect::with_theme(&theme).with_prompt("Which distance algorithm should be used?").items(&["Manhattan (faster)", "Euclidean (more precise)"]).interact()? == 0 {
            DistanceAlgorithm::Manhattan
        } else {
            DistanceAlgorithm::Euclidean
        };
        let output: String = Input::with_theme(&theme).with_prompt("What should the output file be?").interact()?;
        let output_px_size = Input::with_theme(&theme).with_prompt("What should the virtual pixel size be for the output?").interact()?;

        Ok(Self {
            input,
            output: PathBuf::from(output),
            chunks_per_dimension,
            closeness_threshold,
            output_px_size,
            algorithm
        })
    }
}

fn main() -> anyhow::Result<()> {
    let Args {
        input, output, chunks_per_dimension, closeness_threshold, output_px_size, algorithm
    } = Args::parse()?;

    let image = ImageReader::open(input)?.decode()?;
    println!("Image read in");

    let av_px_colours = get_palette(&image, chunks_per_dimension, closeness_threshold, algorithm);
    println!("Palette generated with {} colours", av_px_colours.len());
    let output_img = convert_to_palette(&image, &av_px_colours, algorithm, output_px_size);
    println!("Output image generated");

    output_img.save(&output)?;

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(output).spawn()?;
    }

    Ok(())
}


fn get_palette(image: &DynamicImage, chunks_per_dimension: u32, closeness_threshold: u32, dist_algo: DistanceAlgorithm) -> Vec<Rgb<u8>> {
    let (width, height) = image.dimensions();
    let chunks_per_dimension = chunks_per_dimension.min(width).min(height);
    let (width_chunk_size, height_chunk_size) = (width / chunks_per_dimension, height / chunks_per_dimension);

    let max_num_colours = chunks_per_dimension * chunks_per_dimension;
    let progress_bar = ProgressBar::new(max_num_colours as u64);
    let mut av_px_colours = Vec::with_capacity(max_num_colours as usize);
    for chunk_x in 0..chunks_per_dimension {
        for chunk_y in 0..chunks_per_dimension {

            let mut map: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y).to_rgb();

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

fn convert_to_palette (input: &DynamicImage, palette: &[Rgb<u8>], distance_algorithm: DistanceAlgorithm, scaling_factor: u32) -> DynamicImage {
    let (width, height) = input.dimensions();

    let (num_width_chunks, num_height_chunks) = (width / scaling_factor, height / scaling_factor);
    let mut output = DynamicImage::new(width, height, ColorType::Rgb8);

    let chunks_progress_bar = ProgressBar::new((num_width_chunks * num_height_chunks) as u64);

    for chunk_x in 0..num_width_chunks {
        for chunk_y in 0..num_height_chunks {
            let (mut accum_r, mut accum_g, mut accum_b) = (0_u64, 0_u64, 0_u64);

            for px_x in (scaling_factor * chunk_x)..(scaling_factor * (chunk_x + 1)) {
                for px_y in (scaling_factor * chunk_y)..(scaling_factor * (chunk_y + 1)) {
                    let [r, g, b] = input.get_pixel(px_x, px_y).to_rgb().0;
                    accum_r += r as u64;
                    accum_g += g as u64;
                    accum_b += b as u64;
                }
            }

            let divisor = (scaling_factor * scaling_factor) as u64;

            let av_px = Rgb([(accum_r / divisor) as u8, (accum_g / divisor) as u8, (accum_b / divisor) as u8]);
            let chosen_new_colour = palette.iter().copied().min_by_key(|rgb| {
                distance_algorithm.distance(rgb, &av_px)
            }).unwrap().to_rgba();

            for px_x in (scaling_factor * chunk_x)..(scaling_factor * (chunk_x + 1)) {
                for px_y in (scaling_factor * chunk_y)..(scaling_factor * (chunk_y + 1)) {
                    output.put_pixel(px_x, px_y, chosen_new_colour);
                }
            }

            chunks_progress_bar.inc(1);
        }
    }

    output
}
