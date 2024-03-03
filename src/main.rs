mod header;
mod buffer_modify;

use core::panic;
use std::{borrow::BorrowMut, fs::{self, File}, io::{self, stdout, BufWriter, Cursor, Read, Stdout, Write}, os::fd::{AsFd, AsRawFd}, path::Path};
use bincode::config;
use colored::*;
use clap::{Parser, Subcommand};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageOutputFormat};

use crate::buffer_modify::{convert_dynamic_image_to_png_image, PngImage, ReadImageBinary, WriteImageBinary};
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
        out: Option<String>
    },
    /// Read a hidden message from a PNG Image and output to stdout
    #[command(visible_aliases=["d", "dec"])]
    Decode {
        /// The Path to the image you want to decode. If this is not set, the image will be read from STDIN instead.
        #[arg(short, long)]
        source: Option<String>
    },
    /// Try to get a hidden header from a PNG Image
    #[command(visible_aliases=["s"])]
    Stat {

    }
}



fn main() {
    let cli = Cli {
        verbose: false,
        command: Commands::Encode { source: "Test".to_string(), message: None, out: None }
    };
    let cli = Cli::parse();


    match cli.command {
        Commands::Encode { source, message, out } => {

            let source_path = Path::new(source.as_str());

            if !source_path.exists() {
                eprintln!("Provided path {} does not exist", source.yellow());
                panic!("Path does not exist")
            }


            let mut image = image::open(source_path).map_err(|x| format!("Failed to load the image. You might find more info below: {}", x)).unwrap();
            

            let color_space = image.color();
            let channels = image.color().channel_count();
            let bytes_per_channel = image.color().bytes_per_pixel()/channels;
            let dimensions = image.dimensions();

            let pixel_count = dimensions.0 as u64 * dimensions.1 as u64;

            let image: &mut dyn PngImage = convert_dynamic_image_to_png_image(&mut image).unwrap();

            eprintln!("Loaded image. Contains {} × {} = {}px", dimensions.0, dimensions.1, pixel_count);
            eprintln!("Channels: {}, Bytes per Channel: {}", channels, bytes_per_channel);
            
            let mut message_buf: Vec<u8> = Vec::new();
            let message_copy_result = match message {
                Some(val) => {
                    let data = val.as_bytes();
                    message_buf.write(data).map_err(|err| format!("{}", err))
                },
                None => {
                
                    eprintln!("Waiting for stdin to finish. If you are stuck here, you forgot to pipe a message. You can get a message in by:");
                    eprintln!("- Piping a file or text, e.g. cat mySecret.tgz | ...");
                    eprintln!("- Typing the message now, then sending EOF (usually Ctrl-D)");
                    eprintln!("Alternatively, provide the message via the --message option");
                    eprintln!("Ctrl-C to abort.");
                    io::stdin().read_to_end(&mut message_buf).map_err(|err| format!("{}", err.to_string().red()))
                }
            };
            
            let buf_len: usize = message_copy_result.unwrap();
            eprintln!("Message received and is {} bytes long", buf_len);

            // Define a Header
            let header = generate_v1_header(pixel_count, buf_len as u64, color_space).unwrap();
            let header_binary = {
                let as_raw_header: HeaderRaw = header.try_into().unwrap();

                let as_binary_data = bincode::encode_to_vec(as_raw_header, config::standard()).unwrap();
                as_binary_data
            };

            let (write_mask, start_offset) = match header {
                header::VersionedHeader::V1 { stuffing_opts, data_mask, data_len } => match stuffing_opts {
                    header::V1DataStuffingOptions::None { start_offset } => {
                        assert_eq!(data_len, buf_len as u64);
                        (data_mask, start_offset)
                    },
                },
            };
            
            image.write_data_with_mask(
                &header_binary, write_mask, 0 
            );
            image.write_data_with_mask(&message_buf, write_mask, start_offset as usize);

            let data = image.save_to_png_buffer();
            
        },
        Commands::Decode { source } => {

        },
        Commands::Stat {  } => {
            println!("not implemented")
        }
    }
    
}

fn write_to_stdout(image: image::DynamicImage) -> () {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = BufWriter::new(&mut cursor);
        image.write_to(&mut writer, ImageOutputFormat::Png);
    }
    let mut result_vec: Vec<u8> = Vec::new();
    cursor.read_to_end(&mut result_vec);
    stdout().write(&result_vec);
}

fn write_to_file(image: image::DynamicImage, path: &str) -> () {
    let file = File::create(path).unwrap();
    let mut writer = BufWriter::new(file);
    image.write_to(&mut writer, ImageOutputFormat::Png);
}