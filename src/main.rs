use std::collections::HashMap;
use std::hint::black_box;
use anyhow::bail;
use image::{ColorType, DynamicImage, GenericImage, GenericImageView, ImageReader, Pixel, Rgb};

fn main() -> anyhow::Result<()> {
    println!("reading image");
    let image = ImageReader::open({
        let Some(image) = std::env::args().nth(1) else {
            bail!("unable to find image argument");
        };
        image
    })?.decode()?;
    println!("decoded image");

    println!("getting palette");
    let av_px_colours = get_palette(&image, 6);
    println!("found palette");

    let sf = 32;
    convert_to_palette(&image, &av_px_colours, euclidean_distance, sf)?;
    convert_to_palette(&image, &av_px_colours, manhattan_distance, sf)?;
    convert_to_palette(&image, &av_px_colours, sum_diff, sf)?;
    convert_to_palette(&image, &av_px_colours, prod_diff, sf)?;

    Ok(())
}


fn get_palette(image: &DynamicImage, chunks_per_dimension: u32) -> Vec<Rgb<u8>> {
    let (width, height) = image.dimensions();
    let (width_chunk_size, height_chunk_size) = (width / chunks_per_dimension, height / chunks_per_dimension);

    let mut av_px_colours = Vec::with_capacity((chunks_per_dimension * chunks_per_dimension) as usize);
    for chunk_x in 0..chunks_per_dimension {
        println!("\tprocessing x chunk {}", chunk_x + 1);
        for chunk_y in 0..chunks_per_dimension {

            let mut map: HashMap<_, u32> = HashMap::new();
            for px_x in (width_chunk_size * chunk_x)..(width_chunk_size * (chunk_x + 1)) {
                for px_y in (height_chunk_size * chunk_y)..(height_chunk_size * (chunk_y + 1)) {
                    let px = image.get_pixel(px_x, px_y).to_rgb();

                    const THRESHOLD: u32 = 50;
                    let mut too_close = false;
                    for so_far in &av_px_colours {
                        if euclidean_distance(&px, so_far) < THRESHOLD {
                            too_close = true;
                            break;
                        }
                    }

                    if !too_close {
                        *map.entry(px).or_default() += 1;
                    }
                }
            }

            let (most_common, _) = map.into_iter().max_by_key(|(_, count)| *count).unwrap();
            av_px_colours.push(most_common);
        }
    }

    av_px_colours
}

fn convert_to_palette (input: &DynamicImage, palette: &[Rgb<u8>], dist_func: impl Fn(&Rgb<u8>, &Rgb<u8>) -> u32, scaling_factor: u32) -> anyhow::Result<()> {
    let (width, height) = input.dimensions();

    let fn_name = std::any::type_name_of_val(&dist_func);
    println!("starting to convert with {fn_name}");

    let (num_width_chunks, num_height_chunks) = (width / scaling_factor, height / scaling_factor);
    let mut looks_pixely = DynamicImage::new(width, height, ColorType::Rgb8);

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
                dist_func(rgb, &av_px)
            }).unwrap().to_rgba();

            for px_x in (scaling_factor * chunk_x)..(scaling_factor * (chunk_x + 1)) {
                for px_y in (scaling_factor * chunk_y)..(scaling_factor * (chunk_y + 1)) {
                    looks_pixely.put_pixel(px_x, px_y, chosen_new_colour);
                }
            }

        }
    }

    println!("finished conversion, saving");
    // looks_pixely.save(format!("{fn_name}.jpg"))?;
    black_box(looks_pixely);
    Ok(())
}

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

#[inline]
fn sum_diff(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let a = *r as u32 + *g as u32 + *b as u32;
    let b = *cmp_r as u32 + *cmp_g as u32 + *cmp_b as u32;

    a.abs_diff(b)
}

#[inline]
fn prod_diff(Rgb([r, g, b]): &Rgb<u8>, Rgb([cmp_r, cmp_g, cmp_b]): &Rgb<u8>) -> u32 {
    let a = *r as u32 * *g as u32 * *b as u32;
    let b = *cmp_r as u32 * *cmp_g as u32 * *cmp_b as u32;

    a.abs_diff(b)
}
