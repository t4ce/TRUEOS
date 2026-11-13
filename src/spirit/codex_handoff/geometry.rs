//! Pixel geometry shared by both sides of the UI4 / hardware-cursor handoff.
pub const TILE: i32 = 64;
pub const CANVAS: i32 = 256;
// Runtime bottom-centering shifts the original203px crop6px to the right.
pub const CHEST: Point = Point { x: 113, y: 156 };

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point { pub x: i32, pub y: i32 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pose { pub icon: Point, pub cursor: Point }

impl Pose {
    pub fn inside(self, x: usize, y: usize) -> Option<(usize, usize)> {
        let x = i64::from(self.icon.x) + x as i64 - i64::from(self.cursor.x);
        let y = i64::from(self.icon.y) + y as i64 - i64::from(self.cursor.y);
        ((0..256).contains(&x) && (0..256).contains(&y)).then_some((x as usize,y as usize))
    }
}

pub fn slide(start: Point, cursor: Point, elapsed: u64, duration: u64) -> Pose {
    let t = elapsed.min(duration.max(1));
    let d = duration.max(1);
    // Smoothstep; integer rounding is identical for UI4 and cursor pixels.
    let q = t * 1024 / d;
    let eased = q*q*(3072-2*q)/(1024*1024);
    let axis = |s: i32, c: i32, chest: i32| {
        let target = i64::from(c)+i64::from(chest);
        (i64::from(s)+(target-i64::from(s))*eased as i64/1024)
            .clamp(i64::from(i32::MIN),i64::from(i32::MAX)) as i32
    };
    Pose { icon:Point{x:axis(start.x,cursor.x,CHEST.x),y:axis(start.y,cursor.y,CHEST.y)},cursor }
}

/// Straight RGBA source-over for the owned CPU source, not a GPU destination.
pub fn over(dst: &mut [u8], src: &[u8]) {
    let sa=u32::from(src[3]); let da=u32::from(dst[3]);
    let alpha=sa*255+da*(255-sa);
    if alpha==0 { dst.fill(0); return; }
    for c in 0..3 {
        dst[c]=((u32::from(src[c])*sa*255+u32::from(dst[c])*da*(255-sa)+alpha/2)/alpha) as u8;
    }
    dst[3]=((alpha+127)/255) as u8;
}

pub fn cursor_part(pose: Pose, icon: &[u8], dst: &mut [u8]) {
    for y in 0..64 { for x in 0..64 {
        if let Some((dx,dy))=pose.inside(x,y) {
            over(&mut dst[(dy*256+dx)*4..][..4],&icon[(y*64+x)*4..][..4]);
        }
    }}
}
pub fn ui4_part(pose: Pose, icon: &[u8], dst: &mut [u8], pitch: usize) {
    dst.fill(0);
    for y in 0..64 { for x in 0..64 {
        if pose.inside(x,y).is_none() {
            let s=&icon[(y*64+x)*4..][..4];let d=&mut dst[y*pitch+x*4..][..4];
            for c in 0..3 { d[c]=((u32::from(s[c])*u32::from(s[3])+127)/255) as u8; }
            d[3]=s[3];
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_directions_partition_every_pixel_once() {
        let cursor=Point{x:300,y:200};
        for (dx,dy) in [(-80,80),(280,80),(80,-80),(80,280),(-40,-40),(240,-40),(-40,240),(240,240)] {
            for t in 0..=700 {
                let pose=slide(Point{x:cursor.x+dx,y:cursor.y+dy},cursor,t,700);
                for y in 0..64 {for x in 0..64 {
                    let inside=pose.inside(x,y);
                    let sx=pose.icon.x+x as i32;let sy=pose.icon.y+y as i32;
                    assert_eq!(inside.is_some(),(300..556).contains(&sx)&&(200..456).contains(&sy));
                    if let Some((lx,ly))=inside {assert_eq!((sx,sy),(300+lx as i32,200+ly as i32));}
                }}
            }
        }
    }
    #[test]
    fn live_destination_and_exact_chest() {
        let start=Point{x:-100,y:700};let cursor=Point{x:10,y:20};
        assert_eq!(slide(start,cursor,0,600).icon,start);
        assert_eq!(slide(start,cursor,600,600).icon,Point{x:123,y:176});
        assert_eq!(slide(start,Point{x:100,y:200},600,600).icon,Point{x:213,y:356});
    }
    #[test]
    fn straight_alpha_and_gray_tile_survive() {
        let mut d=[0,0,0,0];over(&mut d,&[128,128,128,102]);assert_eq!(d,[128,128,128,102]);
        let mut opaque=[100,50,20,255];over(&mut opaque,&[200,100,50,128]);
        assert_eq!(opaque,[150,75,35,255]);
        let mut icon=alloc::vec![0;64*64*4];icon[..4].copy_from_slice(&[128,128,128,102]);
        let p=Pose{icon:Point{x:-1,y:0},cursor:Point::default()};
        let mut out=alloc::vec![0;64*64*4];ui4_part(p,&icon,&mut out,256);
        assert_eq!(&out[..4],&[51,51,51,102]);
    }
}

