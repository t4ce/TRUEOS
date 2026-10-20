#!/usr/bin/env python3
"""Native-cap geometry regression, calibrated against the physical R capture."""
import math
import unittest

HALF = 4.0
RADIUS = 8.0 * 0.5

def trims(p, u):
    half = HALF * max(abs(u[0]),abs(u[1]))
    radius = min(RADIUS,half)
    along = p[0]*u[0]+p[1]*u[1]
    across = abs(-p[0]*u[1]+p[1]*u[0])
    return (0 <= along < radius and half-radius < across <= half+1e-9
            and (across-(half-radius))**2+(radius-along)**2 > radius**2+1e-9)

class NativeEndGeometry(unittest.TestCase):
    def test_axis_aligned_coverage_matches_original_rounding(self):
        for u in [(1,0),(-1,0),(0,1),(0,-1)]:
            for x in range(-20,21):
                for y in range(-20,21):
                    p = (x/2,y/2)
                    along = p[0]*u[0]+p[1]*u[1]
                    across = abs(-p[0]*u[1]+p[1]*u[0])
                    old = (0 <= along < RADIUS and HALF-RADIUS < across <= HALF and
                           (across-(HALF-RADIUS))**2+(RADIUS-along)**2 > RADIUS**2+1e-9)
                    self.assertEqual(trims(p,u),old)

    def test_oblique_circle_is_tangent_to_actual_side_and_cap(self):
        for degrees in range(360):
            u = (math.cos(math.radians(degrees)),math.sin(math.radians(degrees)))
            n = (-u[1],u[0])
            half = HALF*max(abs(u[0]),abs(u[1]))
            for side in [-1,1]:
                radius = min(RADIUS,half)
                centre = tuple(radius*a+side*(half-radius)*b for a,b in zip(u,n))
                self.assertAlmostEqual(sum(a*b for a,b in zip(centre,u)),radius)
                self.assertAlmostEqual(half-side*sum(a*b for a,b in zip(centre,n)),radius)
                self.assertFalse(trims(tuple(a*100 for a in u),u))

    def test_actual_r_foot_pixels_round_on_both_sides(self):
        u = (-math.sqrt(0.5),-math.sqrt(0.5))
        # Local frame pixels from the untrimmed hardware capture, centre (456,136).
        removed=[]
        for y in range(125,140):
            for x in range(445,461):
                d=(x+0.5-456,y+0.5-136)
                # Native top-left cap coverage excludes samples on the cap itself.
                if d[0]*u[0]+d[1]*u[1] > 1e-9 and trims(d,u):
                    removed.append((x,y))
        self.assertEqual(removed,[(456,132),(457,133),(452,136),(453,137)])

if __name__ == '__main__':
    unittest.main()
