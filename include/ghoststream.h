/**
 * GhostStream - Core NVIDIA GPU Video Engine
 *
 * C ABI for integration with OBS, FFmpeg, GStreamer, and other applications.
 *
 * Usage:
 *   1. Initialize: ghoststream_init()
 *   2. Create encoder: ghoststream_encoder_create()
 *   3. Encode frames: ghoststream_encode_frame()
 *   4. Cleanup: ghoststream_encoder_destroy(), ghoststream_deinit()
 */

#ifndef GHOSTSTREAM_H
#define GHOSTSTREAM_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Version information */
#define GHOSTSTREAM_VERSION_MAJOR 0
#define GHOSTSTREAM_VERSION_MINOR 1
#define GHOSTSTREAM_VERSION_PATCH 0
#define GHOSTSTREAM_VERSION_STRING "0.1.0"

/* ============================================================================
 * Error Codes
 * ============================================================================ */

typedef enum {
    GHOSTSTREAM_OK = 0,
    GHOSTSTREAM_ERROR_NOT_INITIALIZED = -1,
    GHOSTSTREAM_ERROR_NVENC_NOT_AVAILABLE = -2,
    GHOSTSTREAM_ERROR_CUDA_INIT_FAILED = -3,
    GHOSTSTREAM_ERROR_ENCODER_CREATE_FAILED = -4,
    GHOSTSTREAM_ERROR_INVALID_CONFIG = -5,
    GHOSTSTREAM_ERROR_ENCODE_FAILED = -6,
    GHOSTSTREAM_ERROR_OUT_OF_MEMORY = -7,
    GHOSTSTREAM_ERROR_DEVICE_NOT_FOUND = -8,
    GHOSTSTREAM_ERROR_UNSUPPORTED_CODEC = -9,
    GHOSTSTREAM_ERROR_UNSUPPORTED_RESOLUTION = -10,
    GHOSTSTREAM_ERROR_CAPTURE_FAILED = -11,
} ghoststream_error_t;

/* ============================================================================
 * Codec Types
 * ============================================================================ */

typedef enum {
    GHOSTSTREAM_CODEC_H264 = 0,
    GHOSTSTREAM_CODEC_HEVC = 1,
    GHOSTSTREAM_CODEC_AV1 = 2,
} ghoststream_codec_t;

/* ============================================================================
 * Rate Control Modes
 * ============================================================================ */

typedef enum {
    GHOSTSTREAM_RC_CBR = 0,      /* Constant bitrate */
    GHOSTSTREAM_RC_VBR = 1,      /* Variable bitrate */
    GHOSTSTREAM_RC_CQP = 2,      /* Constant QP */
} ghoststream_rc_mode_t;

/* ============================================================================
 * Presets (P1 = fastest, P7 = best quality)
 * ============================================================================ */

typedef enum {
    GHOSTSTREAM_PRESET_P1 = 1,   /* Fastest */
    GHOSTSTREAM_PRESET_P2 = 2,
    GHOSTSTREAM_PRESET_P3 = 3,
    GHOSTSTREAM_PRESET_P4 = 4,   /* Default/balanced */
    GHOSTSTREAM_PRESET_P5 = 5,
    GHOSTSTREAM_PRESET_P6 = 6,
    GHOSTSTREAM_PRESET_P7 = 7,   /* Best quality */
} ghoststream_preset_t;

/* ============================================================================
 * Pixel Formats
 * ============================================================================ */

typedef enum {
    GHOSTSTREAM_PIXFMT_NV12 = 0,     /* Y plane + interleaved UV (most common) */
    GHOSTSTREAM_PIXFMT_YUV420P = 1,  /* Planar YUV 4:2:0 */
    GHOSTSTREAM_PIXFMT_RGBA = 2,     /* RGBA 8-bit */
    GHOSTSTREAM_PIXFMT_BGRA = 3,     /* BGRA 8-bit */
    GHOSTSTREAM_PIXFMT_ARGB10 = 4,   /* 10-bit ARGB (HDR) */
    GHOSTSTREAM_PIXFMT_P010 = 5,     /* 10-bit NV12 (HDR) */
} ghoststream_pixfmt_t;

/* ============================================================================
 * Structures
 * ============================================================================ */

/**
 * GPU device information
 */
typedef struct {
    uint32_t index;
    char name[256];
    uint32_t compute_major;
    uint32_t compute_minor;
    uint64_t vram_mb;
    bool supports_h264;
    bool supports_hevc;
    bool supports_av1;
    bool supports_10bit;
    bool dual_encoder;
    uint32_t max_width;
    uint32_t max_height;
} ghoststream_gpu_info_t;

/**
 * Encoder configuration
 */
typedef struct {
    ghoststream_codec_t codec;
    uint32_t width;
    uint32_t height;
    uint32_t framerate_num;
    uint32_t framerate_den;
    uint32_t bitrate_kbps;
    uint32_t max_bitrate_kbps;
    ghoststream_rc_mode_t rc_mode;
    ghoststream_preset_t preset;
    ghoststream_pixfmt_t pixel_format;
    uint32_t gop_length;
    uint8_t bframes;
    uint8_t lookahead;
    bool low_latency;
    uint32_t gpu_index;
} ghoststream_encoder_config_t;

/**
 * Frame timing information
 */
typedef struct {
    int64_t pts;        /* Presentation timestamp (microseconds) */
    int64_t dts;        /* Decode timestamp (microseconds) */
    int64_t duration;   /* Frame duration (microseconds) */
    bool is_keyframe;
} ghoststream_frame_timing_t;

/**
 * Encoded packet
 */
typedef struct {
    uint8_t* data;
    size_t size;
    ghoststream_frame_timing_t timing;
    ghoststream_codec_t codec;
    bool is_config;     /* Contains SPS/PPS/VPS */
} ghoststream_packet_t;

/**
 * Encoder statistics
 */
typedef struct {
    uint64_t frames_encoded;
    uint64_t frames_dropped;
    double avg_encode_time_ms;
    double avg_bitrate_kbps;
    uint64_t bytes_encoded;
} ghoststream_encoder_stats_t;

/* Opaque encoder handle */
typedef struct ghoststream_encoder* ghoststream_encoder_t;

/* ============================================================================
 * Library Initialization
 * ============================================================================ */

/**
 * Initialize GhostStream library
 * @return GHOSTSTREAM_OK on success
 */
ghoststream_error_t ghoststream_init(void);

/**
 * Deinitialize GhostStream library
 */
void ghoststream_deinit(void);

/**
 * Get library version string
 */
const char* ghoststream_get_version(void);

/**
 * Check if NVENC is available
 */
bool ghoststream_nvenc_available(void);

/* ============================================================================
 * GPU Detection
 * ============================================================================ */

/**
 * Get number of NVIDIA GPUs with NVENC support
 * @return Number of GPUs, 0 if none found
 */
uint32_t ghoststream_get_gpu_count(void);

/**
 * Get GPU information
 * @param index GPU index
 * @param info Output GPU info structure
 * @return GHOSTSTREAM_OK on success
 */
ghoststream_error_t ghoststream_get_gpu_info(uint32_t index, ghoststream_gpu_info_t* info);

/* ============================================================================
 * Encoder API
 * ============================================================================ */

/**
 * Create default encoder configuration
 * @param config Output configuration structure
 */
void ghoststream_encoder_config_default(ghoststream_encoder_config_t* config);

/**
 * Create an encoder
 * @param config Encoder configuration
 * @param encoder Output encoder handle
 * @return GHOSTSTREAM_OK on success
 */
ghoststream_error_t ghoststream_encoder_create(
    const ghoststream_encoder_config_t* config,
    ghoststream_encoder_t* encoder
);

/**
 * Destroy an encoder
 * @param encoder Encoder handle
 */
void ghoststream_encoder_destroy(ghoststream_encoder_t encoder);

/**
 * Encode a single frame
 * @param encoder Encoder handle
 * @param frame_data Frame pixel data
 * @param frame_size Size of frame data in bytes
 * @param timing Frame timing information
 * @param packet Output encoded packet (data must be freed by caller)
 * @return GHOSTSTREAM_OK on success, packet may be NULL if encoder is buffering
 */
ghoststream_error_t ghoststream_encode_frame(
    ghoststream_encoder_t encoder,
    const uint8_t* frame_data,
    size_t frame_size,
    const ghoststream_frame_timing_t* timing,
    ghoststream_packet_t* packet
);

/**
 * Flush encoder (get remaining frames)
 * @param encoder Encoder handle
 * @param packet Output encoded packet
 * @return GHOSTSTREAM_OK on success, packet is NULL when flush complete
 */
ghoststream_error_t ghoststream_encoder_flush(
    ghoststream_encoder_t encoder,
    ghoststream_packet_t* packet
);

/**
 * Get encoder statistics
 * @param encoder Encoder handle
 * @param stats Output statistics
 */
void ghoststream_encoder_get_stats(
    ghoststream_encoder_t encoder,
    ghoststream_encoder_stats_t* stats
);

/**
 * Free packet data allocated by encode functions
 * @param packet Packet to free
 */
void ghoststream_packet_free(ghoststream_packet_t* packet);

/* ============================================================================
 * Utility Functions
 * ============================================================================ */

/**
 * Get error description string
 * @param error Error code
 * @return Human-readable error description
 */
const char* ghoststream_error_string(ghoststream_error_t error);

/**
 * Calculate required buffer size for a frame
 * @param width Frame width
 * @param height Frame height
 * @param format Pixel format
 * @return Buffer size in bytes
 */
size_t ghoststream_frame_buffer_size(
    uint32_t width,
    uint32_t height,
    ghoststream_pixfmt_t format
);

#ifdef __cplusplus
}
#endif

#endif /* GHOSTSTREAM_H */
