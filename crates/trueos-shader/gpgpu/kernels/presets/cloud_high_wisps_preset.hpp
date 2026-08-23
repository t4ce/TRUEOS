#ifndef TRUEOS_CLOUD_HIGH_WISPS_PRESET_HPP
#define TRUEOS_CLOUD_HIGH_WISPS_PRESET_HPP

// Compile-time mirror of cloud-high-wisps.json (SHA-256:
// 18a5f738e35bdf53cf52c1ed6aec06d6e0c02c9fea1a1b08704fedf3eb23b5d8).
// Keep this header and its JSON source together: the C++ for OpenCL bakery
// records this header as a hashed compiler dependency in the published
// artifact manifest.
namespace trueos::gpgpu::cloud_high_wisps_preset {

inline constexpr float formation_amount = 0.58f;
inline constexpr float formation_seed = 19.37f;
inline constexpr float wind_speed = 1.95f;
inline constexpr float wind_direction = 5.0f;
inline constexpr float turbulence = 0.84f;
inline constexpr float tearing = 1.41f;
inline constexpr float rotation = -0.44f;
inline constexpr float art_sculpt = 0.84f;
inline constexpr float art_outline = 0.76f;
inline constexpr float art_curl = 1.09f;
inline constexpr float art_ribbon = 0.86f;
inline constexpr float art_moon_size = 0.155f;
inline constexpr float art_moon_glow = 0.92f;
inline constexpr float art_grain = 0.13f;
inline constexpr uint art_bands = 6u;

} // namespace trueos::gpgpu::cloud_high_wisps_preset

#endif // TRUEOS_CLOUD_HIGH_WISPS_PRESET_HPP
