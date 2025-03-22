use kornia_image::{Image, ImageError};
use kornia_tensor_ops::TensorOps;
use rayon::prelude::*;

use crate::filter::gaussian_blur;
use crate::filter::spatial_gradient_float;

fn _get_kernel_size(sigma: f32) -> usize {
    let mut ksize = (2.0 * 4.0 * sigma + 1.0) as usize;

    // matches OpenCV, but may cause padding problem for small images
    // PyTorch does not allow to pad more than original size.
    // Therefore there is a hack in forward function
    if ksize % 2 == 0 {
        ksize += 1;
    }

    ksize
}
///Compute the Shi-Tomasi cornerness function.
///
/// The Shi-Tomasi cornerness function is computed as the minimum eigenvalue of the gradient matrix.
///
/// Args:
///    src: The source image.
///   dst: The destination image.
pub fn gftt_response(src: &Image<f32, 1>, dst: &mut Image<f32, 1>) -> Result<(), ImageError> {
    if src.size() != dst.size() {
        return Err(ImageError::InvalidImageSize(
            src.cols(),
            src.rows(),
            dst.cols(),
            dst.rows(),
        ));
    }
    let mut dx = Image::from_size_val(src.size(), 0.0)?;
    let mut dy = Image::from_size_val(src.size(), 0.0)?;
    let _ = spatial_gradient_float(src, &mut dx, &mut dy);
    let dx2: Image<f32, 1> = Image(dx.mul(&dx).unwrap());
    let dy2: Image<f32, 1> = Image(dy.mul(&dy).unwrap());
    let dxy: Image<f32, 1> = Image(dx.mul(&dy).unwrap());

    let mut dx2_g = Image::from_size_val(src.size(), 0.0)?;
    let mut dy2_g = Image::from_size_val(src.size(), 0.0)?;
    let mut dxy_g = Image::from_size_val(src.size(), 0.0)?;
    let _ = gaussian_blur(&dx2, &mut dx2_g, (7usize, 7usize), (1.0, 1.0));
    let _ = gaussian_blur(&dy2, &mut dy2_g, (7usize, 7usize), (1.0, 1.0));
    let _ = gaussian_blur(&dxy, &mut dxy_g, (7usize, 7usize), (1.0, 1.0));

    let det_m = dx2_g
        .mul(&dy2_g)
        .unwrap()
        .sub(&dxy_g.mul(&dxy_g).unwrap())
        .unwrap();
    let trace_m = dx2_g.add(&dy2_g).unwrap();

    let e1 = trace_m
        .add(
            &(trace_m
                .mul(&trace_m)
                .unwrap()
                .sub(&det_m.mul_scalar(4.0))
                .unwrap()
                .abs())
            .powf(0.5),
        )
        .unwrap()
        .mul_scalar(0.5);
    let e2 = trace_m
        .sub(
            &(trace_m
                .mul(&trace_m)
                .unwrap()
                .sub(&det_m.mul_scalar(4.0))
                .unwrap()
                .abs())
            .powf(0.5),
        )
        .unwrap()
        .mul_scalar(0.5);

    let score = e1.min(&e2).unwrap();
    dst.as_slice_mut()
        .iter_mut()
        .zip(score.as_slice().iter())
        .for_each(|(dst_pixel, score_pixel)| {
            *dst_pixel = *score_pixel;
        });
    Ok(())
}

/// Compute the Hessian response of an image.
///
/// The Hessian response is computed as the absolute value of the determinant of the Hessian matrix.
///
/// Args:
///     src: The source image with shape (H, W).
///     dst: The destination image with shape (H, W).
pub fn hessian_response(src: &Image<f32, 1>, dst: &mut Image<f32, 1>) -> Result<(), ImageError> {
    if src.size() != dst.size() {
        return Err(ImageError::InvalidImageSize(
            src.cols(),
            src.rows(),
            dst.cols(),
            dst.rows(),
        ));
    }

    let src_data = src.as_slice();

    dst.as_slice_mut()
        .par_chunks_exact_mut(src.cols())
        .enumerate()
        .for_each(|(row_idx, row_chunk)| {
            if row_idx == 0 || row_idx == src.rows() - 1 {
                // skip the first and last row
                return;
            }

            let row_offset = row_idx * src.cols();

            row_chunk
                .iter_mut()
                .enumerate()
                .for_each(|(col_idx, dst_pixel)| {
                    if col_idx == 0 || col_idx == src.cols() - 1 {
                        // skip the first and last column
                        return;
                    }

                    let current_idx = row_offset + col_idx;
                    let prev_row_idx = current_idx - src.cols();
                    let next_row_idx = current_idx + src.cols();

                    let v11 = src_data[prev_row_idx - 1];
                    let v12 = src_data[prev_row_idx];
                    let v13 = src_data[prev_row_idx + 1];
                    let v21 = src_data[current_idx - 1];
                    let v22 = src_data[current_idx];
                    let v23 = src_data[current_idx + 1];
                    let v31 = src_data[next_row_idx - 1];
                    let v32 = src_data[next_row_idx];
                    let v33 = src_data[next_row_idx + 1];

                    let dxx = v21 - 2.0 * v22 + v23;
                    let dyy = v12 - 2.0 * v22 + v32;
                    let dxy = 0.25 * (v31 - v11 - v33 + v13);

                    let det = dxx * dyy - dxy * dxy;

                    *dst_pixel = det;
                });
        });

    Ok(())
}

/// Compute the DoG response of an image.
///
/// The DoG response is computed as the difference of the Gaussian responses of two images.
///
/// Args:
///     src: The source image with shape (H, W).
///     dst: The destination image with shape (H, W).
///     sigma1: The sigma of the first Gaussian kernel.
///     sigma2: The sigma of the second Gaussian kernel.
pub fn dog_response(
    src: &Image<f32, 1>,
    dst: &mut Image<f32, 1>,
    sigma1: f32,
    sigma2: f32,
) -> Result<(), ImageError> {
    if src.size() != dst.size() {
        return Err(ImageError::InvalidImageSize(
            src.cols(),
            src.rows(),
            dst.cols(),
            dst.rows(),
        ));
    }

    let mut gauss1 = Image::from_size_val(src.size(), 0.0)?;
    let mut gauss2 = Image::from_size_val(src.size(), 0.0)?;
    let ks1 = _get_kernel_size(sigma1);
    let ks2 = _get_kernel_size(sigma2);

    gaussian_blur(src, &mut gauss1, (ks1, ks1), (sigma1, sigma1))?;
    gaussian_blur(src, &mut gauss2, (ks2, ks2), (sigma2, sigma2))?;

    let gauss1_data = gauss1.as_slice();
    let gauss2_data = gauss2.as_slice();
    let dst_data = dst.as_slice_mut();

    dst_data
        .iter_mut()
        .zip(gauss2_data.iter().zip(gauss1_data.iter()))
        .for_each(|(dst_pixel, (gauss2_pixel, gauss1_pixel))| {
            *dst_pixel = gauss2_pixel - gauss1_pixel;
        });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gftt_response() -> Result<(), ImageError> {
        #[rustfmt::skip]
        let src = Image::from_size_slice(
            [9, 9].into(),
            &[
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
                ],
        )?;

        let mut dst = Image::from_size_val([9, 9].into(), 0.0)?;
        gftt_response(&src, &mut dst)?;

        #[rustfmt::skip]
        let expected_center_value = 0.1274;
        assert!(
            (dst.as_slice()[4 * 9 + 4] - expected_center_value).abs() < 1e-4,
            "Center value should be close to expected value"
        );
        let max = dst
            .as_slice()
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        assert!(
            (*max - expected_center_value).abs() < 1e-4,
            "Max value should be close to centre value"
        );
        Ok(())
    }
    #[test]
    fn test_hessian_response() -> Result<(), ImageError> {
        #[rustfmt::skip]
        let src = Image::from_size_slice(
            [5, 5].into(),
            &[
                0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 1.0, 0.0, 1.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        )?;

        let mut dst = Image::from_size_val([5, 5].into(), 0.0)?;
        hessian_response(&src, &mut dst)?;

        #[rustfmt::skip]
        assert_eq!(
            dst.as_slice(),
            &[
                0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 4.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0,
            ]
        );
        Ok(())
    }

    #[test]
    fn test_dog_response() -> Result<(), ImageError> {
        #[rustfmt::skip]
        let src = Image::from_size_slice(
            [5, 5].into(),
            &[
                0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 1.0, 1.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        )?;

        let mut dst = Image::from_size_val([5, 5].into(), 0.0)?;

        let sigma1 = 0.5;
        let sigma2 = 1.0;

        dog_response(&src, &mut dst, sigma1, sigma2)?;

        let center_value = dst.as_slice()[2 * 5 + 2];
        let expected_center_value = -0.2195;
        assert!(
            (center_value - expected_center_value).abs() < 1e-4,
            "Center value should be close to expected value"
        );

        let sum: f32 = dst.as_slice().iter().sum();
        let expected_sum = -0.7399;
        assert!(
            (sum - expected_sum).abs() < 1e-4,
            "Sum of DoG response should be close to expected value"
        );

        Ok(())
    }
}
