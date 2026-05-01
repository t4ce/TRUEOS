// src/mnist_loader.rs
use ndarray::prelude::*;
use mnist::{Mnist, MnistBuilder};
use std::fs;
use std::path::Path;

pub struct MnistData {
    pub train_images: Array4<f32>,
    pub train_labels: Array4<f32>,
    pub test_images: Array4<f32>,
    pub test_labels: Array4<f32>,
}

pub fn load_data() -> MnistData {
    // 检查数据是否存在
    let data_dir = r"C:\Users\chen-\Downloads\ts_rsnn-master\ts_rsnn-master\data";
    if !Path::new(data_dir).exists() {
        fs::create_dir(data_dir).unwrap_or_else(|e| {
            panic!("Failed to create 'data' directory. Please create it manually: {}", e);
        });
        println!("Created 'data' directory.");
        println!("Please download the 4 MNIST .gz files and place them in the 'data' directory.");
        panic!("MNIST data files not found in 'data/' directory.");
    }

    println!("Loading MNIST data from {}...", data_dir);

    let Mnist {
        trn_img,
        trn_lbl,
        tst_img,
        tst_lbl,
        ..
    } = MnistBuilder::new()
        .label_format_digit()
        .training_set_length(60_000)
        .validation_set_length(0) 
        .test_set_length(10_000)
        .base_path(data_dir) 
        .finalize();

    let train_size = 60_000;
    let train_images = process_images(trn_img, train_size);
    let train_labels = process_labels(trn_lbl, train_size);

    let test_size = 10_000;
    let test_images = process_images(tst_img, test_size);
    let test_labels = process_labels(tst_lbl, test_size);

    MnistData {
        train_images,
        train_labels,
        test_images,
        test_labels,
    }
}

fn process_images(data: Vec<u8>, size: usize) -> Array4<f32> {
    let data_f32: Vec<f32> = data.into_iter().map(|x| x as f32 / 255.0).collect();
    Array4::from_shape_vec((size, 1, 28, 28), data_f32)
        .expect("Error reshaping images")
}

fn process_labels(labels: Vec<u8>, size: usize) -> Array4<f32> {
    let mut one_hot = Array4::<f32>::zeros((size, 1, 1, 10));
    for (i, &label) in labels.iter().enumerate() {
        one_hot[[i, 0, 0, label as usize]] = 1.0;
    }
    one_hot
}