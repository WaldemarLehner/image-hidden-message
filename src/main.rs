mod buffer_modify;
mod header;

use clap::{Parser, Subcommand};
use colored::*;
use core::panic;
use header::{try_get_header, VersionedHeader};
use image::GenericImageView;
use std::{
    fs::File,
    io::{self, stdout, BufWriter, Read, Write},
    path::Path,
    process::exit,
};

use crate::buffer_modify::{convert_dynamic_image_to_png_image, PngImage};
use crate::header::{generate_v1_header, HeaderRaw};

#[derive(Parser)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Write a hidden message to a PNG Image
    #[command(visible_aliases=["e", "enc"])]
    Encode {
        /// Path to the image you want to encode the message into
        source: String,
        /// The message you want to hide. If this is not set, the message will be read from STDIN instead. The message can be binary.
        #[arg(short, long)]
        message: Option<String>,
        /// The output path of the modified Image. If this is not set, the message will be written to STDOUT.
        #[arg(short, long)]
        out: Option<String>,
    },
    /// Read a hidden message from a PNG Image and output to stdout
    #[command(visible_aliases=["d", "dec"])]
    Decode {
        /// The Path to the image you want to decode. If this is not set, the image will be read from STDIN instead.
        #[arg(short, long)]
        source: Option<String>,
    },
    /// Try to get a hidden header from a PNG Image
    #[command(visible_aliases=["s"])]
    Stat {},
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Encode {
            source,
            message,
            out,
        } => {
            let source_path = Path::new(source.as_str());

            if !source_path.exists() {
                eprintln!("Provided path {} does not exist", source.yellow());
                panic!("Path does not exist")
            }

            let mut image = image::open(source_path)
                .map_err(|x| {
                    format!(
                        "Failed to load the image. You might find more info below: {}",
                        x
                    )
                })
                .unwrap();

            let color_space = image.color();
            let channels = image.color().channel_count();
            let bytes_per_channel = image.color().bytes_per_pixel() / channels;
            let dimensions = image.dimensions();

            let pixel_count = dimensions.0 as u64 * dimensions.1 as u64;

            let image: &mut dyn PngImage = convert_dynamic_image_to_png_image(&mut image).unwrap();

            eprintln!(
                "Loaded image. Contains {} × {} = {}px",
                dimensions.0, dimensions.1, pixel_count
            );
            eprintln!(
                "Channels: {}, Bytes per Channel: {}",
                channels, bytes_per_channel
            );

            let mut message_buf: Vec<u8> = Vec::new();
            let message_copy_result = match message {
                Some(val) => {
                    let data = val.as_bytes();
                    message_buf.write(data).map_err(|err| format!("{}", err))
                }
                None => {
                    eprintln!("Waiting for stdin to finish. If you are stuck here, you forgot to pipe a message. You can get a message in by:");
                    eprintln!("- Piping a file or text, e.g. cat mySecret.tgz | ...");
                    eprintln!("- Typing the message now, then sending EOF (usually Ctrl-D)");
                    eprintln!("Alternatively, provide the message via the --message option");
                    eprintln!("Ctrl-C to abort.");
                    io::stdin()
                        .read_to_end(&mut message_buf)
                        .map_err(|err| format!("{}", err.to_string().red()))
                }
            };

            let buf_len: usize = message_copy_result.unwrap();
            eprintln!("Message received and is {} bytes long", buf_len);

            // Define a Header
            let header = generate_v1_header(pixel_count, buf_len as u64, color_space).unwrap();
            let header_binary = {
                let mut as_raw_header: HeaderRaw = header.try_into().unwrap();

                let mut as_binary_data = Vec::new();
                // Magic
                as_binary_data.push(as_raw_header.magic);
                // Header Len
                as_binary_data.push((as_raw_header.header_len >> 8 & 0xFF) as u8);
                as_binary_data.push((as_raw_header.header_len & 0xFF) as u8);
                // Data
                as_binary_data.append(&mut as_raw_header.data);
                // CRC
                for i in 0..4 {
                    as_binary_data.push((as_raw_header.crc >> ((3 - i) * 8) & 0xFF) as u8)
                }
                //
                as_binary_data
            };

            let (write_mask, start_offset) = match header {
                header::VersionedHeader::V1 {
                    stuffing_opts,
                    data_mask,
                    data_len,
                } => match stuffing_opts {
                    header::V1DataStuffingOptions::None { start_offset } => {
                        assert_eq!(data_len, buf_len as u64);
                        (data_mask, start_offset)
                    }
                },
            };

            image.write_data_with_mask(&header_binary, 0b1u64 << 63 >> 7, 0);
            image.write_data_with_mask(&message_buf, write_mask, start_offset as usize);

            let mut data = image.save_to_png_buffer().unwrap();

            let out = match out {
                Some(x) => {
                    if x == "-" {
                        None
                    } else {
                        Some(x)
                    }
                }
                None => None,
            };

            eprint!("len: {}", data.len());

            match out {
                None => stdout().write_all(&mut data).unwrap(),
                Some(path) => {
                    let file = File::create(path).unwrap();
                    let mut writer = BufWriter::new(file);
                    writer.write_all(&mut data).unwrap();
                }
            }

            eprintln!("...done")
        }
        Commands::Decode { source } => {
            let mut image = (match source {
                Some(path) => {
                    let source_path = Path::new(path.as_str());

                    if !source_path.exists() {
                        eprintln!("Provided path {} does not exist", path.yellow());
                        panic!("Path does not exist")
                    }
                    image::open(path)
                },
                None => {
                    let mut message_buf = Vec::new();
                    eprintln!("Waiting for stdin to finish. If you are stuck here, you forgot to pipe a PNG file. You can fix this by");
                    eprintln!("- Piping a PNG file, e.g. cat imgWithSecret.png | ...");
                    eprintln!("Alternatively, provide the source via the --source option");
                    eprintln!("Ctrl-C to abort.");
                    io::stdin()
                        .read_to_end(&mut message_buf)
                        .map_err(|err| format!("{}", err.to_string().red()))
                        .unwrap();
                    image::load_from_memory_with_format(&message_buf, image::ImageFormat::Png)
                }
            }).map_err(|x| x.to_string()).unwrap();

            let image: &mut dyn PngImage = convert_dynamic_image_to_png_image(&mut image).unwrap();

            let header = match try_get_header(image) {
                Ok(val) => val,
                Err(err) => {
                    eprintln!("Failed to parse Header: {}", err);
                    exit(1);
                }
            };

            let payload = match header {
                VersionedHeader::V1 {
                    stuffing_opts,
                    data_mask,
                    data_len,
                } => {
                    let start_offset = match stuffing_opts {
                        header::V1DataStuffingOptions::None { start_offset } => start_offset,
                    };

                    image.read_data_with_mask(data_mask, start_offset as usize, data_len as usize)
                }
            };

            stdout().write(&payload).unwrap();
        }
        Commands::Stat {} => {
            let mut image = {
                let mut message_buf = Vec::new();
                eprintln!("Waiting for stdin to finish. If you are stuck here, you forgot to pipe a PNG file. You can fix this by");
                eprintln!("- Piping a PNG file, e.g. cat imgWithSecret.png | ...");
                eprintln!("Ctrl-C to abort.");
                io::stdin()
                    .read_to_end(&mut message_buf)
                    .map_err(|err| format!("{}", err.to_string().red()))
                    .unwrap();
                image::load_from_memory_with_format(&message_buf, image::ImageFormat::Png).unwrap()
            };
            let image: &mut dyn PngImage = convert_dynamic_image_to_png_image(&mut image).unwrap();

            match try_get_header(image) {
                Ok(val) => match val {
                    VersionedHeader::V1 {
                        stuffing_opts,
                        data_mask,
                        data_len,
                    } => {
                        eprintln!("--------------------------");
                        println!("Success: {}", "yes".green());
                        match stuffing_opts {
                            header::V1DataStuffingOptions::None { start_offset } => {
                                println!("Pixel Offset: {}", start_offset)
                            }
                        };
                        println!("Byte Length: {}", data_len);
                        println!("Data Mask: {:#066b}", data_mask);
                        println!("         :  |0      |8      |16     |24     |32     |40     |48     |56     |64");
                    }
                },
                Err(err) => {
                    println!("Success: {}", "no".red());
                    println!("Reason: {}", err.italic());
                }
            };
        }
    }
}
