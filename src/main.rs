use dialoguer::theme::ColorfulTheme;
use dialoguer::{FuzzySelect, Input};
use image::{ColorType, DynamicImage, GenericImage, GenericImageView, ImageReader, Pixel, Rgba};
use indicatif::ProgressBar;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Args {
    input: PathBuf,
    output: PathBuf,
    chunks_per_dimension: u32,
    closeness_threshold: u32,
    output_px_size: u32,
    algorithm: DistanceAlgorithm,
}

#[derive(Debug, Copy, Clone)]
pub enum DistanceAlgorithm {
    Euclidean,
    Manhattan,
}

impl DistanceAlgorithm {
    fn distance(&self, a: &Rgba<u8>, b: &Rgba<u8>) -> u32 {
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

impl Args {
    pub fn parse() -> anyhow::Result<Self> {
        match Self::parse_env() {
            Some(x) => Ok(x),
            None => Self::parse_manual(),
        }
    }

    fn parse_env() -> Option<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();

        if args.len() == 0 {
            return None;
        }
        if args.len() == 1 {
            let first =args[0].to_lowercase();
            if ["--help", "-help", "-h", "--h", "help", "h", "?", "-?"].contains(&first.as_str()) {
                eprintln!("usage: pxls [input_file] [chunks_per_dimension] [closeness_threshold] [distance_algo] [output_file] [output_virtual_pixel_size]");
                std::process::exit(1);
            }
        }

        let Ok(
            [input, chunks_per_dimension, closeness_threshold, algorithm, output, output_px_size],
        ): Result<[String; 6], _> = args.try_into()
        else {
            return None;
        };

        let input = PathBuf::from(input);
        if !input.exists() || !input.is_file() {
            eprintln!("[input_file] must be a file that exists");
            return None;
        }

        let Ok(chunks_per_dimension) = chunks_per_dimension.parse() else {
            eprintln!("[chunks_per_dimension] must be a valid u32");
            return None;
        };
        let Ok(closeness_threshold) = closeness_threshold.parse() else {
            eprintln!("[closeness_threshold] must be a valid u32");
            return None;
        };
        let algorithm = match algorithm.to_lowercase().as_str() {
            "euclidean" => DistanceAlgorithm::Euclidean,
            "manhattan" => DistanceAlgorithm::Manhattan,
            _ => {
                eprintln!("[distance_algo] must be either `euclidean` or `manhattan`");
                return None;
            }
        };

        let output = PathBuf::from(output);
        let Ok(output_px_size) = output_px_size.parse() else {
            eprintln!("[output_virtual_pixel_size] must be a valid u32");
            return None;
        };

        Some(Self {
            input,
            chunks_per_dimension,
            closeness_threshold,
            algorithm,
            output,
            output_px_size
        })
    }

    fn parse_manual() -> anyhow::Result<Self> {
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
        let chunks_per_dimension = Input::with_theme(&theme)
            .with_prompt("How many chunks per dimension should be used for palette generation?")
            .interact()?;
        let closeness_threshold = Input::with_theme(&theme)
            .with_prompt("What should the closeness threshold be for palette generation?")
            .interact()?;
        let algorithm = if FuzzySelect::with_theme(&theme)
            .with_prompt("Which distance algorithm should be used?")
            .items(&["Manhattan (faster)", "Euclidean (more precise)"])
            .interact()?
            == 0
        {
            DistanceAlgorithm::Manhattan
        } else {
            DistanceAlgorithm::Euclidean
        };
        let output: String = Input::with_theme(&theme)
            .with_prompt("What should the output file be?")
            .interact()?;
        let output_px_size = Input::with_theme(&theme)
            .with_prompt("What should the virtual pixel size be for the output?")
            .interact()?;

        Ok(Self {
            input,
            output: PathBuf::from(output),
            chunks_per_dimension,
            closeness_threshold,
            output_px_size,
            algorithm,
        })
    }
}

fn main() -> anyhow::Result<()> {
    let Args {
        input,
        output,
        chunks_per_dimension,
        closeness_threshold,
        output_px_size,
        algorithm,
    } = Args::parse()?;

    let image = ImageReader::open(input)?.decode()?;
    println!("Image read in");

    //tyvm https://stackoverflow.com/questions/26885198/find-closest-factor-to-a-number-of-a-number
    let get_closest_factor = |target, number| {
        for i in 0..number {
            if number % (target + i) == 0 {
                return target + i;
            } else if number % (target - i) == 0 {
                return target - i;
            }
        }
        return number;
    };

    let output_px_size = get_closest_factor(image.width(), output_px_size);

    println!("Generating palette");
    let av_px_colours = get_palette(&image, chunks_per_dimension, closeness_threshold, algorithm);
    println!("Palette generated with {} colours", av_px_colours.len());
    println!("Converting image to palette & shrinking");
    let output_img = dither_palette(&image, &av_px_colours, algorithm, output_px_size);
    println!("Output image generated");

    output_img.save(&output)?;

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(output).spawn()?;
    }

    Ok(())
}

fn get_palette(
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


fn dither_palette(
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

            chunks_progress_bar.inc(1);
        }
    }

    output
}
