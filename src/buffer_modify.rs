use std::io::{BufWriter, Cursor, Read};

use image::{ColorType, DynamicImage, ImageBuffer, ImageOutputFormat};

pub(crate) trait WriteImageBinary {
    fn write_data_with_mask(&mut self, data: &[u8], writing_mask: u64, pixel_offset: usize);
}

pub(crate) trait ReadImageBinary {
    fn read_data_with_mask(&self, data: &[u8], reading_mask: u64, pixel_offset: usize, length: usize) -> Vec<u8>;
}

pub(crate) trait PngImageSaveable {
    fn save_to_png_buffer(&self) -> Vec<u8>;
}

impl ReadImageBinary for ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    fn read_data_with_mask(&self, data: &[u8], reading_mask: u64, pixel_offset: usize, length: usize) -> Vec<u8> {
        let image_buf =self.as_raw();

        read_from_buffer(
            image_buf, 
            pixel_offset, 
            length, 
            reading_mask, 
            ColorType::Rgb8,
        )
    }
}

impl WriteImageBinary for ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    fn write_data_with_mask(&mut self, data: &[u8], writing_mask: u64, pixel_offset: usize) {
        let image_buf = self.as_raw();
        write_to_buffer(
            &image_buf, 
            pixel_offset, 
            writing_mask, 
            ColorType::Rgb8, 
            data
        )
    }
}

impl PngImageSaveable for ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    fn save_to_png_buffer(&self) -> Vec<u8> {
        let mut cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut writer = BufWriter::new(&mut cursor);
            self.write_to(&mut writer, ImageOutputFormat::Png);
        }
        let mut result_vec: Vec<u8> = Vec::new();
        cursor.read_to_end(&mut result_vec);

        result_vec
    }
}

pub(crate) trait PngImage: ReadImageBinary + WriteImageBinary + PngImageSaveable {}
impl<T> PngImage for T where T: ReadImageBinary + WriteImageBinary + PngImageSaveable {}


pub(crate) fn convert_dynamic_image_to_png_image(image: &mut DynamicImage) -> Result<&mut dyn PngImage, String> {
    match image.color() {
        image::ColorType::L8 | image::ColorType::La8 |  image::ColorType::L16 |  image::ColorType::La16 
            => Err("Luma-type Images are currently not supported".to_string()),
        image::ColorType::Rgb8 => {
            Ok(image.as_mut_rgb8().unwrap() as &mut dyn PngImage)
        },
        //image::ColorType::Rgba8 => Ok(image.as_mut_rgba8().unwrap()),
        //image::ColorType::Rgb16 => Ok(image.as_mut_rgb16().unwrap()),
        //image::ColorType::Rgba16 => Ok(image.as_rgba16().unwrap()),
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => Err("Floating-Type Images are currently not supported".to_string()),
        _ => Err("Not implemented".to_string()),
    }
    
}



///
/// read_mask is a right-padded mask defining which bits in a pixel are relevant.
pub(crate) fn read_from_buffer(image_buf: &[u8], pixels_offset_start: usize, bytes_len_read: usize, read_mask: u64, color_type: ColorType) -> Vec<u8> {
    let read_mask_vectorized = vectorize_bit_mask(read_mask);
    let mut return_buffer: Vec<u8> = Vec::with_capacity(bytes_len_read);
    let mut bytes_to_fill = bytes_len_read;
    
    let channel_count: u8 = color_type.channel_count();
    let channel_size_bytes: u8 = color_type.bytes_per_pixel() / channel_count;

    // Out data
    let mut current_byte: Vec<bool> = Vec::with_capacity(8);

    let pixel_len_bytes = channel_count * channel_size_bytes;
    let mut current_pixel_index = pixels_offset_start;
    while bytes_to_fill > 0 {
        // This is a pixel as u8 slice. 
        // This has variable length based on the pixel used (e.g rgb8 vs rgba8)
        let current_pixel_slice = &image_buf[current_pixel_index * pixel_len_bytes as usize..(current_pixel_index+1)*pixel_len_bytes as usize];
        
        for (byte_id, byte) in current_pixel_slice.iter().enumerate() {
            let read_mask_for_this_byte = read_mask_vectorized[byte_id];
            for bit_index in 0..8 as usize {
                let position_mask = 0b0000_0001u8 << (7-bit_index);
                
                if read_mask_for_this_byte & position_mask == 0 {
                    // Current Bit is not "relevant". It doesnt contain data. So we skip it
                    continue;
                }

                let bit_value = byte & position_mask != 0;
                current_byte.push(bit_value);

                if current_byte.len() > 7 {
                    let joined_byte = build_byte(&current_byte);
                    return_buffer.push(joined_byte);
                    current_byte.clear();
                    bytes_to_fill -= 1;
                }
            }
        }
        current_pixel_index += 1;
    }

    return_buffer


}


fn update_pixel_slice(image_buf: &Vec<u8>, pixel_len_bytes: u8, current_pixel_index: usize) -> &mut [u8] {
    &mut image_buf[current_pixel_index * pixel_len_bytes as usize..(current_pixel_index+1)*pixel_len_bytes as usize]
}

pub(crate) fn write_to_buffer(image_buf: &Vec<u8>, pixels_offset_start: usize, write_mask: u64, color_type: ColorType, data_to_write: &[u8]) {

    let pixel_len_bytes = color_type.bytes_per_pixel();

    let mut current_in_pixel_index = 0;
    let mut current_pixel_index = pixels_offset_start;
    

    // This is a pixel as u8 slice. 
    // This has variable length based on the pixel used (e.g rgb8 vs rgba8)
    let mut current_pixel_slice = update_pixel_slice(image_buf, pixel_len_bytes, current_pixel_index);

    for byte_u8_write_data in data_to_write {
        let current_byte = unbuild_byte(&byte_u8_write_data);
        for bit in current_byte {
            // First find the next in_pixel index that can have data written to it. This is done with a bitmask.
            let pixel_mask = 0b1u64 << (64 - current_in_pixel_index);
            if write_mask & pixel_mask == 0 {
                // Current bit inside the pixel is not marked for writing. It shall be skipped.
                current_in_pixel_index += 1;
                if current_in_pixel_index > pixel_len_bytes * 8 {
                    // We are finished with this pixel.
                    // Reset in-pixel index, increment pixel index, and get view into new slice.
                    current_in_pixel_index = 0;
                    current_pixel_index += 1;
                    current_pixel_slice = update_pixel_slice(image_buf, pixel_len_bytes, current_pixel_index);
                }

                continue;
            } 
            // First "clear" the value bit to 0 by AND-ing the byte we want to modify with an inverted mask
            let index_of_byte_in_pixel_slice = (current_in_pixel_index / 8) as usize;
            //  0b0000_0001 << 7
            //  0b1000_0000 >> current_in_pixel_index % 8 (e.g. 5)
            //  0b0000_0100 !
            let selector_mask = 0b1 << 7 >> (current_in_pixel_index % 8);
            //  0b0000_0100 !
            //                        v------------->v
            //  0b1111_1011 & 0bABCD_EFGH => 0bABCD_E0GH 
            current_pixel_slice[index_of_byte_in_pixel_slice] &= !selector_mask;

            if bit {  
                //         v------------->v
                // 0bABCD_E0GH => 0bABCD_E1GH   
                current_pixel_slice[index_of_byte_in_pixel_slice] |= selector_mask;
            }

            // if here: the pixel buffer was modified.
        }
    }    
}


pub(crate) fn vectorize_bit_mask(mut read_mask: u64) -> Vec<u8> {
    let mut return_data: Vec<u8> = Vec::with_capacity(8);
    for _ in 0..8 {
        let byte = (read_mask & 0xFF) as u8;
        return_data.push(byte);
        read_mask >>= 8
    }

    return_data.reverse();
    return_data
}

pub(crate) fn unvectorize_bit_mask(read_mask: Vec<u8>) -> u64 {
    if read_mask.len() != 64/8 {
        panic!("Unexpected length of mask. MUST have length of {}.", 64/8)
    }

    let mut return_data = 0u64;
    for entry in read_mask {
        return_data <<= 8;
        return_data |= entry as u64;
    }

    return_data
}

fn build_byte(current_byte: &Vec<bool>) -> u8 {
    let mut return_byte = 0u8;
    for i in 0..8 {
        return_byte <<= 1;
        if current_byte[7-i] {
            return_byte |= 0b1u8;
        }

    }

    return_byte
}

fn unbuild_byte(byte: &u8) -> Vec<bool> {
    let mut return_vec = Vec::with_capacity(8);
    for i in 0..8 {
        let mask = 0b1000_0000u8 >> i;
        return_vec.push(byte & mask != 0)
    }

    return_vec
}
