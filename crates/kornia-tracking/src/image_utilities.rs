//! Image utilities used by the kornia-tracking modules.
//! This module contains functions to compute image gradients, check bounds,
//! perform the SE(2) exponential map, and detect keypoints.

use image::{GenericImageView, GrayImage};
use imageproc::corners::{corners_fast9, Corner};
use glam::{Mat3, Vec3};

/// Computes image gradients at (x, y).
pub fn image_grad(grayscale_image: &GrayImage, x: f32, y: f32) -> Vec3 {
    let ix = x.floor() as u32;
    let iy = y.floor() as u32;
    let dx = x - ix as f32;
    let dy = y - iy as f32;
    let ddx = 1.0 - dx;
    let ddy = 1.0 - dy;

    let px0y0 = grayscale_image.get_pixel(ix, iy).0[0] as f32;
    let px1y0 = grayscale_image.get_pixel(ix + 1, iy).0[0] as f32;
    let px0y1 = grayscale_image.get_pixel(ix, iy + 1).0[0] as f32;
    let px1y1 = grayscale_image.get_pixel(ix + 1, iy + 1).0[0] as f32;
    let res0 = ddx * ddy * px0y0 + ddx * dy * px0y1 + dx * ddy * px1y0 + dx * dy * px1y1;

    let pxm1y0 = grayscale_image.get_pixel(ix - 1, iy).0[0] as f32;
    let pxm1y1 = grayscale_image.get_pixel(ix - 1, iy + 1).0[0] as f32;
    let res_mx = ddx * ddy * pxm1y0 + ddx * dy * pxm1y1 + dx * ddy * px0y0 + dx * dy * px0y1;

    let px2y0 = grayscale_image.get_pixel(ix + 2, iy).0[0] as f32;
    let px2y1 = grayscale_image.get_pixel(ix + 2, iy + 1).0[0] as f32;
    let res_px = ddx * ddy * px1y0 + ddx * dy * px1y1 + dx * ddy * px2y0 + dx * dy * px2y1;
    let res1 = 0.5 * (res_px - res_mx);

    let px0ym1 = grayscale_image.get_pixel(ix, iy - 1).0[0] as f32;
    let px1ym1 = grayscale_image.get_pixel(ix + 1, iy - 1).0[0] as f32;
    let res_my = ddx * ddy * px0ym1 + ddx * dy * px0y0 + dx * ddy * px1ym1 + dx * dy * px1y0;

    let px0y2 = grayscale_image.get_pixel(ix, iy + 2).0[0] as f32;
    let px1y2 = grayscale_image.get_pixel(ix + 1, iy + 2).0[0] as f32;
    let res_py = ddx * ddy * px0y1 + ddx * dy * px0y2 + dx * ddy * px1y1 + dx * dy * px1y2;
    let res2 = 0.5 * (res_py - res_my);

    Vec3::new(res0, res1, res2)
}

/// Checks if a keypoint is within the acceptable boundaries of the image.
pub fn point_in_bound(keypoint: &Corner, height: u32, width: u32, radius: u32) -> bool {
    keypoint.x >= radius
        && keypoint.x <= width - radius
        && keypoint.y >= radius
        && keypoint.y <= height - radius
}

/// Checks if a coordinate (x, y) is within image bounds, considering a radius.
pub fn inbound(image: &GrayImage, x: f32, y: f32, radius: u32) -> bool {
    let x = x.round() as u32;
    let y = y.round() as u32;
    x >= radius && y >= radius && x < image.width() - radius && y < image.height() - radius
}

/// Computes the SE(2) exponential map based on a Vec3 input.
pub fn se2_exp_matrix(a: &Vec3) -> Mat3 {
    let theta = a.z;
    let (sin_theta_by_theta, one_minus_cos_theta_by_theta) = if theta.abs() < f32::EPSILON {
        let theta_sq = theta * theta;
        (1.0 - theta_sq / 6.0, 0.5 * theta - theta * theta_sq / 24.0)
    } else {
        (theta.sin() / theta, (1.0 - theta.cos()) / theta)
    };

    let t_x = sin_theta_by_theta * a.x - one_minus_cos_theta_by_theta * a.y;
    let t_y = one_minus_cos_theta_by_theta * a.x + sin_theta_by_theta * a.y;

    Mat3::from_cols_array(&[
        theta.cos(), -theta.sin(), 0.0,
        theta.sin(),  theta.cos(), 0.0,
        t_x,          t_y,         1.0,
    ])
}

/// Detects keypoints on the image using grid constraints and FAST corner detection.
pub fn detect_key_points(
    image: &GrayImage,
    grid_size: u32,
    current_corners: &Vec<Corner>,
    num_points_in_cell: u32,
) -> Vec<Corner> {
    const EDGE_THRESHOLD: u32 = 19;
    let h = image.height();
    let w = image.width();
    let mut all_corners = vec![];
    let rows = (h / grid_size + 1) as usize;
    let cols = (w / grid_size + 1) as usize;
    let mut grids = vec![vec![0i32; cols]; rows];

    let x_start = (w % grid_size) / 2;
    let x_stop = x_start + grid_size * (w / grid_size - 1) + 1;
    let y_start = (h % grid_size) / 2;
    let y_stop = y_start + grid_size * (h / grid_size - 1) + 1;

    // add existing corners to grid
    for corner in current_corners {
        if corner.x >= x_start
            && corner.y >= y_start
            && corner.x < x_stop + grid_size
            && corner.y < y_stop + grid_size
        {
            let grid_x = ((corner.x - x_start) / grid_size) as usize;
            let grid_y = ((corner.y - y_start) / grid_size) as usize;
            if grid_y < grids.len() && grid_x < grids[grid_y].len() {
                grids[grid_y][grid_x] += 1;
            }
        }
    }

    for x in (x_start..x_stop).step_by(grid_size as usize) {
        for y in (y_start..y_stop).step_by(grid_size as usize) {
            let grid_x = ((x - x_start) / grid_size) as usize;
            let grid_y = ((y - y_start) / grid_size) as usize;
            if grids[grid_y][grid_x] > 0 {
                continue;
            }
            let image_view = image.view(x, y, grid_size, grid_size).to_image();
            let mut points_added = 0;
            let mut threshold: u8 = 40;
            while points_added < num_points_in_cell && threshold >= 10 {
                let mut fast_corners = corners_fast9(&image_view, threshold);
                fast_corners.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
                for mut point in fast_corners {
                    if points_added >= num_points_in_cell {
                        break;
                    }
                    point.x += x;
                    point.y += y;
                    if point_in_bound(&point, h, w, EDGE_THRESHOLD) {
                        all_corners.push(point);
                        points_added += 1;
                    }
                }
                if threshold < 5 {
                    break; // avoid underflow
                }
                threshold -= 5;
            }
        }
    }
    all_corners
}
