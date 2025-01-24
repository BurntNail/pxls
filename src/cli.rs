use std::fs;
use std::path::PathBuf;
use std::process::Command;
use dialoguer::{FuzzySelect, Input};
use dialoguer::theme::ColorfulTheme;
use image::ImageReader;
use crate::logic::{dither_palette, get_palette, DistanceAlgorithm};

#[allow(dead_code)]
pub fn cli_main() -> anyhow::Result<()> {
    let CliArgs {
        input,
        output,
        chunks_per_dimension,
        closeness_threshold,
        output_px_size,
        algorithm,
    } = CliArgs::parse()?;

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

pub struct CliArgs {
    input: PathBuf,
    output: PathBuf,
    chunks_per_dimension: u32,
    closeness_threshold: u32,
    output_px_size: u32,
    algorithm: DistanceAlgorithm,
}

impl CliArgs {
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
