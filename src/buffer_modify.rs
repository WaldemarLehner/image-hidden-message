use std::io::{BufWriter, Cursor, Read};

use image::{ColorType, DynamicImage, ImageBuffer, ImageOutputFormat};

pub(crate) trait WriteImageBinary {
    fn write_data_with_mask(&mut self, data: &[u8], writing_mask: u64, pixel_offset: usize);
}

pub(crate) trait ReadImageBinary {
    fn read_data_with_mask(&self, reading_mask: u64, pixel_offset: usize, length: usize)
        -> Vec<u8>;
}

pub(crate) trait PngImageSaveable {
    fn save_to_png_buffer(&self) -> Result<Vec<u8>, String>;
}

impl ReadImageBinary for ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    fn read_data_with_mask(
        &self,
        reading_mask: u64,
        pixel_offset: usize,
        length: usize,
    ) -> Vec<u8> {
        let image_buf = self.as_raw();

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
        // TODO: Check if we can somehow get "as_raw_mut" of sth like that.
        // Copying the image buffer to be able to do modifications smells a lot.
        let mut image_buf: image::FlatSamples<&mut [u8]> = self.as_flat_samples_mut();

        write_to_buffer(
            &mut image_buf.as_mut_slice(),
            pixel_offset,
            writing_mask,
            ColorType::Rgb8,
            data,
        )
    }
}

impl PngImageSaveable for ImageBuffer<image::Rgb<u8>, Vec<u8>> {
    fn save_to_png_buffer(&self) -> Result<Vec<u8>, String> {
        let mut cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut writer = BufWriter::new(&mut cursor);
            self.write_to(&mut writer, ImageOutputFormat::Png)
                .map_err(|x| x.to_string())?;
        }
        let mut result_vec: Vec<u8> = Vec::new();
        cursor
            .read_to_end(&mut result_vec)
            .map_err(|x| x.to_string())?;

        Ok(result_vec)
    }
}

impl PngImageSaveable for ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    fn save_to_png_buffer(&self) -> Result<Vec<u8>, String> {
        let mut cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        {
            let mut writer = BufWriter::new(&mut cursor);
            self.write_to(&mut writer, ImageOutputFormat::Png)
                .map_err(|x| x.to_string())?;
        }
        Ok(cursor.into_inner())
    }
}

impl ReadImageBinary for ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    fn read_data_with_mask(
        &self,
        reading_mask: u64,
        pixel_offset: usize,
        length: usize,
    ) -> Vec<u8> {
        let image_buf = self.as_raw();

        read_from_buffer(
            image_buf,
            pixel_offset,
            length,
            reading_mask,
            ColorType::Rgba8,
        )
    }
}

impl WriteImageBinary for ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    fn write_data_with_mask(&mut self, data: &[u8], writing_mask: u64, pixel_offset: usize) {
        let mut image_buf: image::FlatSamples<&mut [u8]> = self.as_flat_samples_mut();

        write_to_buffer(
            &mut image_buf.as_mut_slice(),
            pixel_offset,
            writing_mask,
            ColorType::Rgba8,
            data,
        )
    }
}

pub(crate) trait PngImage: ReadImageBinary + WriteImageBinary + PngImageSaveable {}
impl<T> PngImage for T where T: ReadImageBinary + WriteImageBinary + PngImageSaveable {}

pub(crate) fn convert_dynamic_image_to_png_image(
    image: &mut DynamicImage,
) -> Result<&mut dyn PngImage, String> {
    match image.color() {
        image::ColorType::L8
        | image::ColorType::La8
        | image::ColorType::L16
        | image::ColorType::La16 => Err("Luma-type Images are currently not supported".to_string()),
        image::ColorType::Rgb8 => Ok(image.as_mut_rgb8().unwrap() as &mut dyn PngImage),
        image::ColorType::Rgba8 => Ok(image.as_mut_rgba8().unwrap() as &mut dyn PngImage),
        //image::ColorType::Rgb16 => Ok(image.as_mut_rgb16().unwrap()),
        //image::ColorType::Rgba16 => Ok(image.as_rgba16().unwrap()),
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => {
            Err("Floating-Type Images are currently not supported".to_string())
        }
        _ => Err("Not implemented".to_string()),
    }
}

///
/// read_mask is a right-padded mask defining which bits in a pixel are relevant.
pub(crate) fn read_from_buffer(
    image_buf: &[u8],
    pixels_offset_start: usize,
    bytes_len_read: usize,
    read_mask: u64,
    color_type: ColorType,
) -> Vec<u8> {
    let offset_map = create_offset_map(read_mask, color_type.bits_per_pixel() as usize);
    if offset_map.len() == 0 {
        panic!("offset-map is empty. Cannot continue.");
    }

    let mut return_data: Vec<u8> = Vec::new();

    let mut current_byte_vec: Vec<bool> = Vec::with_capacity(8);

    let mut current_pixel_index = pixels_offset_start;
    // Loop over all pixels. This will break out once bytes_len_read is finished
    loop {
        let current_pixel_slice =
            get_pixel_slice(image_buf, color_type.bytes_per_pixel(), current_pixel_index);

        for in_pixel_offset in &offset_map {
            let bit_value = current_pixel_slice[in_pixel_offset / 8]
                & (0b1u8 << 7 >> (in_pixel_offset % 8))
                != 0;
            current_byte_vec.push(bit_value);

            if current_byte_vec.len() == 8 {
                // Now build the byte
                let mut byte = 0u8;
                for i in 0..8 {
                    if !current_byte_vec[i] {
                        continue;
                    }
                    byte |= 0b1 << 7 >> i;
                }
                return_data.push(byte);
                current_byte_vec.clear();
                if return_data.len() == bytes_len_read {
                    return return_data;
                }
            }
        }
        current_pixel_index += 1;
    }
}

pub(crate) fn write_to_buffer(
    image_buf: &mut [u8],
    pixels_offset_start: usize,
    write_mask: u64,
    color_type: ColorType,
    data_to_write: &[u8],
) {
    let offset_map = create_offset_map(write_mask, color_type.bits_per_pixel() as usize);
    if offset_map.len() == 0 {
        panic!("offset-map is empty. Cannot continue.");
    }
    let mut current_byte_to_write: Vec<bool> = Vec::with_capacity(8);
    let mut data_to_write_index = 0usize;
    let mut current_pixel_index = pixels_offset_start;

    let current_byte = data_to_write[data_to_write_index];
    for i in 0..8 {
        current_byte_to_write.push(current_byte & (0b1u8 << 7 >> i) != 0);
    }
    current_byte_to_write.reverse(); // Reversed as we will just "pop" from the back

    loop {
        let current_pixel_slice =
            get_pixel_slice_mut(image_buf, color_type.bytes_per_pixel(), current_pixel_index);

        for in_pixel_offset in &offset_map {
            let local_pixel_offset = in_pixel_offset / 8;
            let local_mask = 0b1u8 << 7 >> in_pixel_offset % 8;
            // inverted mask causes the value bit to be set to 0
            current_pixel_slice[local_pixel_offset] &= !local_mask;
            if current_byte_to_write.pop().unwrap() {
                // set the value bit to 1
                current_pixel_slice[local_pixel_offset] |= local_mask;
            }
            if current_byte_to_write.len() == 0 {
                data_to_write_index += 1;
                if data_to_write_index >= data_to_write.len() {
                    return;
                }
                let current_byte = data_to_write[data_to_write_index];
                for i in 0..8 {
                    current_byte_to_write.push((current_byte & (0b1u8 << 7 >> i)) != 0);
                }
                current_byte_to_write.reverse() // Reversed as we will just "pop" from the back
            }
        }
        current_pixel_index += 1;
    }
}

fn get_pixel_slice(image_buf: &[u8], pixel_len_bytes: u8, current_pixel_index: usize) -> &[u8] {
    &image_buf[current_pixel_index * pixel_len_bytes as usize
        ..(current_pixel_index + 1) * pixel_len_bytes as usize]
}

fn get_pixel_slice_mut(
    image_buf: &mut [u8],
    pixel_len_bytes: u8,
    current_pixel_index: usize,
) -> &mut [u8] {
    &mut image_buf[current_pixel_index * pixel_len_bytes as usize
        ..(current_pixel_index + 1) * pixel_len_bytes as usize]
}

///
/// Returns a vec containing an "offset map" which defines the offsets of all value-bits
fn create_offset_map(write_mask: u64, pixel_size: usize) -> Vec<usize> {
    let mut return_map = Vec::new();
    for i in 0..pixel_size {
        if ((0b1 << 63 >> i) & write_mask) > 0 {
            return_map.push(i)
        }
    }

    return_map
}

#[cfg(test)]
mod tests {
    use rand::RngCore;

    use super::*;

    #[test]
    fn create_offset_map_test() {
        let input =
            0b1000_0100_0010_0001__0000_0000_0000_0000__0000_0000_0000_0000__0000_0000_0000_0001u64;

        let output = create_offset_map(input, 64);

        assert_eq!(output, vec![0usize, 5usize, 10usize, 15usize, 63usize])
    }

    #[test]
    fn encode_and_decode_into_byte_buffer() {
        let mut image_buf = vec![0u8; 200];
        rand::thread_rng().fill_bytes(&mut image_buf);

        let data: Vec<u8> = vec![0x12, 0x34, 0x56, 0x78];
        write_to_buffer(
            &mut image_buf,
            0,
            0x01_01_01_00_00_00_00_00u64,
            ColorType::Rgba8,
            &data,
        );

        let result = read_from_buffer(
            &image_buf,
            0,
            4,
            0x01_01_01_00_00_00_00_00u64,
            ColorType::Rgba8,
        );

        assert_eq!(data, result);
    }
}
