#[cfg(test)]
mod lifecycle_contracts {
    use super::*;
    use super::super::{BlueprintSceneSurface, Rgba8Upload, WindowId};

    fn open(owner: u8, window: u32) -> u64 {
        let resources = ImageResources::new();
        let incarnation = resources.incarnation;
        SURFACES.lock().push(BlueprintSceneSurface {
            owner: WindowOwner::Vm(owner), render_target: window, window: WindowId(window),
            hull_images: resources, sprites: Vec::new(), sprite_upload: None,
            gpu_submission_unretired: false,
        });
        incarnation
    }
    fn close_all() {
        for mut surface in core::mem::take(&mut *SURFACES.lock()) {
            forget_surface(surface.owner, surface.render_target, surface.hull_images.incarnation);
            for (_, BlueprintSpriteSource::Uploaded(source)) in surface.sprites.drain(..) { destroy_rgba8_surface(source); }
            if let Some(upload) = surface.sprite_upload.take() { destroy_rgba8_surface(upload.owned); }
        }
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn owner_incarnation_and_busy_checks_prevent_stale_publication() {
        let incarnation = open(7, 42);
        assert_eq!(authenticate_window(8, 42), Err(ERROR_NOT_FOUND));
        assert_eq!(authenticate_window(7, 43), Err(ERROR_NOT_FOUND));
        let mut prepared = prepare_rgba(1, 1, &[20, 30, 40, 100]).unwrap();
        assert!(!prepared.premultiplied, "dark straight-alpha pixels must not be guessed premultiplied");
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 4096);
        assert_eq!(install_prepared(8, 42, incarnation, 27, &mut prepared), Err(ERROR_NOT_FOUND));
        assert_eq!(install_prepared(7, 42, incarnation + 1, 27, &mut prepared), Err(ERROR_NOT_FOUND));
        {
            let _draw = SURFACES.lock();
            assert_eq!(authenticate_window(7, 42), Err(ERROR_BUSY));
            assert_eq!(install_prepared(7, 42, incarnation, 27, &mut prepared), Err(ERROR_BUSY));
        }
        let id = install_prepared(7, 42, incarnation, 27, &mut prepared).unwrap();
        assert!(id >= SERVICE_SPRITE_BASE);
        assert!(prepared.virt.is_null());
        drop(prepared);
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 4096);
        close_all();
        let fresh = open(7, 42);
        assert_ne!(fresh, incarnation);
        let mut stale = prepare_rgba(1, 1, &[0, 0, 0, 0]).unwrap();
        assert_eq!(install_prepared(7, 42, incarnation, 27, &mut stale), Err(ERROR_NOT_FOUND));
        drop(stale);
        close_all();
    }
    #[test]
    fn cancellation_is_nonblocking_and_quarantined_sources_remain_charged() {
        let incarnation = open(7, 42);
        let mut prepared = prepare_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        let id = install_prepared(7, 42, incarnation, 27, &mut prepared).unwrap();
        {
            let mut draw = SURFACES.lock();
            draw[0].gpu_submission_unretired = true;
            assert_eq!(release_rgba(7, 42, incarnation, id), 0);
            assert_eq!(release_rgba(7, 42, incarnation, id), 0);
            assert_eq!(RETIREMENTS.lock().len(), 1);
        }
        reap_retired();
        assert_eq!(SURFACES.lock()[0].sprites.len(), 1);
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 4096);
        assert_eq!(reserve_image_bytes(MAX_GLOBAL_IMAGE_BYTES), Err(ERROR_BUSY));
        SURFACES.lock()[0].gpu_submission_unretired = false;
        reap_retired();
        assert!(SURFACES.lock()[0].sprites.is_empty());
        assert!(RETIREMENTS.lock().is_empty());
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 0);
        close_all();
    }
    #[test]
    fn pending_guest_upload_va_is_reserved_and_preparation_drop_releases_budget() {
        let incarnation = open(7, 42);
        let mut first = prepare_rgba(1, 1, &[1, 2, 3, 255]).unwrap();
        let first_id = install_prepared(7, 42, incarnation, 1, &mut first).unwrap();
        let upload_gpu = {
            let mut surfaces = SURFACES.lock();
            let BlueprintSpriteSource::Uploaded(source) = surfaces[0].sprites.remove(0).1;
            let gpu = source.surface.gpu;
            surfaces[0].sprite_upload = Some(Rgba8Upload { owned: source });
            gpu
        };
        let mut second = prepare_rgba(1, 1, &[4, 5, 6, 255]).unwrap();
        let second_id = install_prepared(7, 42, incarnation, 2, &mut second).unwrap();
        assert_ne!(first_id, second_id);
        let surfaces = SURFACES.lock();
        assert!(surfaces[0].sprites[0].1.surface().gpu >= upload_gpu + 4096);
        drop(surfaces);
        let temporary = prepare_rgba(1, 1, &[0, 0, 0, 0]).unwrap();
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 3 * 4096);
        drop(temporary);
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 2 * 4096);
        close_all();
    }
    #[test]
    fn global_budget_includes_all_prepared_or_quarantined_bytes() {
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 0);
        reserve_image_bytes(MAX_GLOBAL_IMAGE_BYTES - 4096).unwrap();
        let prepared = prepare_rgba(1, 1, &[0, 0, 0, 0]).unwrap();
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), MAX_GLOBAL_IMAGE_BYTES);
        assert!(matches!(prepare_rgba(1, 1, &[0, 0, 0, 0]), Err(ERROR_BUSY)));
        drop(prepared);
        release_image_bytes(MAX_GLOBAL_IMAGE_BYTES - 4096);
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn globe_extent_preserves_straight_alpha_across_pitched_dma_rows() {
        let incarnation = open(7, 42);
        let (width, height) = (200usize, 183usize);
        let mut rgba = vec![0; width * height * 4];
        for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[(index % 251) as u8, (index / width) as u8, 37, 90]);
        }
        let mut prepared = prepare_rgba(width as u32, height as u32, &rgba).unwrap();
        assert_eq!(prepared.pitch, 832);
        assert!(!prepared.opaque);
        assert!(!prepared.premultiplied);
        assert_eq!(prepared.bytes % 4096, 0);
        let sprite = install_prepared(7, 42, incarnation, 27, &mut prepared).unwrap();
        {
            let surfaces = SURFACES.lock();
            let BlueprintSpriteSource::Uploaded(source) = &surfaces[0].sprites[0].1;
            assert!(source.surface.is_valid());
            let copied = unsafe { core::slice::from_raw_parts(source.virt, source.bytes) };
            for row in 0..height {
                let start = row * source.surface.pitch_bytes as usize;
                assert_eq!(&copied[start..start + width * 4], &rgba[row * width * 4..(row + 1) * width * 4]);
                assert!(copied[start + width * 4..start + source.surface.pitch_bytes as usize].iter().all(|byte| *byte == 0));
            }
        }
        assert_eq!(release_rgba(7, 42, incarnation, sprite), 0);
        close_all();
    }
    #[test]
    fn failed_dma_preparation_releases_budget_and_allows_the_next_image() {
        let incarnation = open(7, 42);
        crate::dma::FAIL_NEXT_ALLOCATION.store(true, Ordering::Relaxed);
        assert!(matches!(prepare_rgba(1, 1, &[1, 2, 3, 255]), Err(ERROR_UI4)));
        assert_eq!(IMAGE_BYTES.load(Ordering::Relaxed), 0);
        let mut prepared = prepare_rgba(1, 1, &[1, 2, 3, 255]).unwrap();
        let id = install_prepared(7, 42, incarnation, 27, &mut prepared).unwrap();
        assert!(id >= SERVICE_SPRITE_BASE);
        close_all();
    }
    #[test]
    fn invalid_dma_surface_is_rejected_without_transferring_ownership() {
        let incarnation = open(7, 42);
        let mut prepared = prepare_rgba(1, 1, &[1, 2, 3, 255]).unwrap();
        prepared.phys += 1;
        assert_eq!(install_prepared(7, 42, incarnation, 27, &mut prepared), Err(ERROR_UI4));
        assert!(!prepared.virt.is_null());
        assert!(SURFACES.lock()[0].sprites.is_empty());
        drop(prepared);
        close_all();
    }
}
