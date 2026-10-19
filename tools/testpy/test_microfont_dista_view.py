#!/usr/bin/env python3
"""Run the production atlas view controls and source-cache identity on the host."""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::view::Config;

    #[test]
    fn initial_view_matches_current_specimen_size_and_fine_meso_geometry() {
        let view = Config::default();
        assert_eq!((view.quality_index,view.size_index,view.aa),(3,4,false));
        assert_eq!((view.segments(),view.scale(),view.sample_scale()),(16,16,1));
        assert_eq!((view.width(),view.height()),(1536,1056));
    }

    #[test]
    fn quality_cycles_visible_polygon_detail_without_changing_view_size_or_aa() {
        let mut view = Config::default();
        let initial = view;
        for expected in [24,4,8,12,16] {
            assert!(view.apply_key('q'));
            assert_eq!(view.segments(),expected);
            assert_eq!((view.scale(),view.aa),(16,false));
        }
        assert_eq!(view,initial);
        assert!(view.apply_key('Q'));
        assert_eq!(view.segments(),24);
    }

    #[test]
    fn size_selection_and_steps_stop_at_both_bounds_without_invalidating_unchanged_views() {
        let mut view = Config::default();
        assert!(!view.apply_key('+'));
        assert!(!view.apply_key('5'));
        for (key,scale) in [('1',1),('2',2),('3',4),('4',8),('5',16)] {
            assert!(view.apply_key(key));
            assert_eq!(view.scale(),scale);
            assert!(!view.apply_key(key));
        }
        for scale in [8,4,2,1] {
            assert!(view.apply_key('-'));
            assert_eq!(view.scale(),scale);
        }
        assert!(!view.apply_key('-'));
        assert!(view.apply_key('+'));
        assert_eq!(view.scale(),2);
    }

    #[test]
    fn aa_changes_source_density_and_cache_key_while_preserving_logical_extent() {
        let mut view = Config::default();
        let logical = (view.width(),view.height());
        assert!(view.apply_key('a'));
        assert_eq!((view.sample_scale(),view.source_scale()),(2,32));
        assert_eq!((view.width(),view.height()),logical);
        assert_eq!((view.width()*view.sample_scale(),view.height()*view.sample_scale()),(3072,2112));
        assert_eq!(view.source_key(),(16,32));
        assert!(view.apply_key('A'));
        assert_eq!(view,Config::default());
    }

    #[test]
    fn different_display_views_share_only_identical_render_sources() {
        let mut small_aa = Config::default();
        small_aa.apply_key('3'); small_aa.apply_key('a');
        let mut large = Config::default(); large.apply_key('4');
        assert_ne!(small_aa,large);
        assert_eq!(small_aa.source_key(),large.source_key());
        assert_ne!((small_aa.width(),small_aa.height()),(large.width(),large.height()));
        small_aa.apply_key('q');
        assert_ne!(small_aa.source_key(),large.source_key());
    }

    #[test]
    fn unrelated_text_is_ignored_and_every_reachable_source_fits_the_r8_extent() {
        let mut view = Config::default();
        for key in ['B','y','%','/','0','6','\n','\u{1b}','🗺'] {
            assert!(!view.apply_key(key)); assert_eq!(view,Config::default());
        }
        for _ in 0..5 {
            for size in '1'..='5' {
                view.apply_key(size);
                for _ in 0..2 {
                    view.apply_key('a');
                    let width=view.width()*view.sample_scale();
                    let height=view.height()*view.sample_scale();
                    assert!(width>0 && width<=3072 && height>0 && height<=2112);
                    assert_eq!(width*height,96*66*view.source_scale()*view.source_scale());
                }
            }
            view.apply_key('q');
        }
    }

    #[test]
    fn error_bound_tracks_geometry_and_logical_size_but_not_aa() {
        let mut view = Config::default();
        let fine = view.max_error_px();
        assert!((fine-0.106999139312119).abs()<1e-12);
        view.apply_key('a'); assert_eq!(view.max_error_px(),fine);
        view.apply_key('1'); assert!((view.max_error_px()*16.0-fine).abs()<1e-12);
        view.apply_key('5');
        let expected = [0.04764016800182978,1.6310059673555894,0.42388464033051965,
            0.1897455355244571,0.106999139312119];
        for error in expected {
            view.apply_key('q'); assert!((view.max_error_px()-error).abs()<1e-12);
        }
    }
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-dista-view-tests-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        production = ROOT / "src/r/services/microfont_dista_view.rs"
        (directory / "src/lib.rs").write_text(
            f'#![allow(dead_code)]\n#[path = {json.dumps(str(production))}] mod view;\n' + TESTS
        )
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-dista-view-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[dependencies]\nlibm="0.2"\n'
            f'microfont={{path={json.dumps(str(ROOT / "vendor/microfont"))},default-features=false}}\n'
        )
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
             "--target", "x86_64-unknown-linux-gnu", "--quiet"], cwd=directory, check=True,
        )


if __name__ == "__main__":
    main()
