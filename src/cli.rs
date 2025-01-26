use pxls::{
    dither_palette, get_palette, DistanceAlgorithm, OutputSettings, PaletteSettings,
};
use anyhow::anyhow;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{FuzzySelect, Input};
use image::ImageReader;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;

#[allow(dead_code)]
pub fn cli_main(should_ask: bool) -> anyhow::Result<()> {
    let CliArgs {
        input,
        output,
        chunks_per_dimension,
        closeness_threshold,
        output_px_size,
        algorithm,
        dithering_factor,
        dithering_scale,
    } = CliArgs::parse(should_ask)?;

    let should_stop = Arc::new(AtomicBool::new(false));

    let image = ImageReader::open(input)?.decode()?;
    println!("Image read in");

    println!("Generating palette");
    let (tx, _rx) = channel();
    let av_px_colours = get_palette(
        &image,
        PaletteSettings {
            chunks_per_dimension,
            closeness_threshold,
        },
        algorithm,
        &tx,
        should_stop.clone()
    );
    println!("Palette generated with {} colours", av_px_colours.len());
    println!("Converting image to palette & shrinking");
    let output_img = dither_palette(
        &image,
        &av_px_colours,
        algorithm,
        OutputSettings {
            output_px_size,
            dithering_likelihood: dithering_factor,
            dithering_scale,
            scale_output_to_original: true, //TODO: consider making this an option...
        },
        &tx,
        should_stop.clone()
    );
    //TODO: maybe the CLI should get fewer options when coming from env
    //TODO: opinionated defaults?
    println!("Output image generated");

    output_img.save(&output)?;

    Ok(())
}

pub struct CliArgs {
    input: PathBuf,
    output: PathBuf,
    chunks_per_dimension: u32,
    closeness_threshold: u32,
    output_px_size: u32,
    algorithm: DistanceAlgorithm,
    dithering_factor: u32,
    dithering_scale: u32,
}

impl CliArgs {
    pub fn parse(should_use_asking: bool) -> anyhow::Result<Self> {
        if should_use_asking {
            Self::parse_manual()
        } else {
            Self::parse_env().ok_or_else(|| anyhow!("unable to parse from args"))
        }
    }

    fn parse_env() -> Option<Self> {
        let args: Vec<String> = std::env::args().skip(1).collect();

        let Ok(
            [input, chunks_per_dimension, closeness_threshold, algorithm, output, output_px_size, dithering_factor, dithering_scale],
        ): Result<[String; 8], _> = args.try_into()
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
        let Ok(dithering_factor) = dithering_factor.parse() else {
            eprintln!("[dithering_factor] must be a valid u32");
            return None;
        };
        let Ok(dithering_scale) = dithering_scale.parse() else {
            eprintln!("[dithering_scale] must be a valid u32");
            return None;
        };

        Some(Self {
            input,
            output,
            chunks_per_dimension,
            closeness_threshold,
            output_px_size,
            algorithm,
            dithering_factor,
            dithering_scale,
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
        let dithering_factor = Input::with_theme(&theme)
            .with_prompt("What should the dithering factor be for the output?")
            .interact()?;
        let dithering_scale = Input::with_theme(&theme)
            .with_prompt("What should the dithering scale be for the output?")
            .interact()?;

        Ok(Self {
            input,
            output: PathBuf::from(output),
            chunks_per_dimension,
            closeness_threshold,
            output_px_size,
            algorithm,
            dithering_factor,
            dithering_scale,
        })
    }
}
