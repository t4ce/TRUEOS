#!/usr/bin/env python3
"""Cross-check the kernel CPU mesher against the optional JS debug meshes."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix='trueos-microfont-runtime-') as directory:
        work = Path(directory)
        (work / 'src').mkdir()
        (work / 'Cargo.toml').write_text('''[package]
name = "trueos-microfont-runtime-test"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
libm = "=0.2.16"
serde_json = "1.0"
microfont = { path = ''' + json.dumps(str(ROOT / 'vendor/microfont')) + ''' }
''')
        (work / 'src/main.rs').write_text(r'''
extern crate alloc;
#[path = "MESH_PATH"] mod mesh;
#[path = "SOURCE_PATH"] mod source;
fn word(b:&[u8],i:usize)->usize { u32::from_le_bytes(b[i..i+4].try_into().unwrap()) as usize }
fn main() {
    let root=std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let config:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("tools/microfonttothemoon/recipe.json")).unwrap()).unwrap();
    let settings=source::Settings {
        mesh:mesh::Recipe { sigma:config["sigma"].as_f64().unwrap(),level:config["level"].as_f64().unwrap(),
            balance:config["balance"].as_f64().unwrap(),detail:config["detail"].as_u64().unwrap() as usize,prune:config["prune"].as_bool().unwrap() },
        reach:config["reach"].as_f64().unwrap(),long_reach_value:config["long_reach_value"].as_f64().unwrap(),
        long_reach:config["long_reach"].as_array().unwrap().iter().map(|v|v.as_str().unwrap().chars().next().unwrap()).collect(),
    };
    // The two runtime reach profiles must generate distinct slash geometry.
    let slash=source::Glyph { character:'/',cells:vec![(microfont::font_pixels(b'/') as u128)<<2] };
    let long=slash.build(&settings).unwrap();
    let short=mesh::build(&slash.points(),6.0,settings.mesh,settings.reach).unwrap();
    assert_ne!(long.positions,short.positions);
    // Direct enumeration preserves the vendored subset, not UI-symbol aliases.
    assert_eq!(microfont::gui_han_characters().len(),512);
    for cp in microfont::gui_han_characters() { assert!(microfont::gui_han_cell_pixels(cp).is_some()); }
    for byte in 0x20u8..=0xff { assert_eq!(microfont::glyph_byte(microfont::glyph_character(byte).unwrap()),byte); }
    for han in [false,true] {
        let folder=root.join(if han {"src/intel/gpu_font/moon/han"} else {"src/intel/gpu_font/moon"});
        let catalog:serde_json::Value=serde_json::from_slice(&std::fs::read(folder.join("catalog.json")).unwrap()).unwrap();
        assert_eq!(catalog["recipe"],config,"debug oracle must be regenerated for the current recipe");
        let glyphs=source::glyphs(han).unwrap();
        for (g,entry) in glyphs.iter().zip(catalog["glyphs"].as_array().unwrap()) {
            assert_eq!(g.character.to_string(),entry["char"].as_str().unwrap());
        }
        let mut total=0;
        for part in 0..4 {
            let actual=source::build_part(han,part,&settings).unwrap();
            let bytes=std::fs::read(folder.join(format!("part{}/draft.mesh",part+1))).unwrap();
            let vertices=word(&bytes,8);let indices=word(&bytes,12);
            assert_eq!(actual.vertices.len(),vertices,"vertex count: han={han} part={part}");
            assert_eq!(actual.indices.len(),indices,"index count: han={han} part={part}");
            let mut max_error=0.0f32;
            for (i,p) in actual.vertices.iter().enumerate() { for axis in 0..3 {
                let expected=f32::from_bits(word(&bytes,16+i*12+axis*4) as u32);
                max_error=max_error.max((p[axis]-expected).abs());
            }}
            assert!(max_error<2e-6,"position error {max_error}: han={han} part={part}");
            for (i,&index) in actual.indices.iter().enumerate() {
                assert_eq!(index as usize,word(&bytes,16+vertices*12+i*4),"index {i}: han={han} part={part}");
            }
            total+=actual.glyph_count;
            assert_eq!(actual.long_reach_count,usize::from(!han && part==0));
            println!("han={han} part={}: {} glyphs, {vertices} vertices, {} triangles; matches debug mesh (max error {max_error})",part+1,actual.glyph_count,indices/3);
        }
        assert_eq!(total,if han {512} else {514});
    }
    println!("Runtime CPU meshes match all eight debug parts; all 1026 glyphs and both reach profiles verified.");
}
'''.replace('MESH_PATH', str(ROOT / 'src/r/services/microfonttothemoon/mesh.rs'))
           .replace('SOURCE_PATH', str(ROOT / 'src/r/services/microfonttothemoon/source.rs')))
        subprocess.run([os.environ.get('CARGO', 'cargo'), 'run', '--offline', '--release', '--', str(ROOT)],
                       cwd=work, check=True)


if __name__ == '__main__':
    main()
