//! NVENC SDK C Bindings for GhostStream
//!
//! Direct bindings to NVIDIA Video Codec SDK (nvEncodeAPI).
//! Supports H.264/AVC, HEVC/H.265, and AV1 encoding.
//!
//! Requires: NVIDIA Video Codec SDK 12.2+, libnvidia-encode.so

const std = @import("std");
const builtin = @import("builtin");

pub const version = "12.2";

// ============================================================================
// NVENC API Version and Constants
// ============================================================================

pub const NVENCAPI_MAJOR_VERSION = 12;
pub const NVENCAPI_MINOR_VERSION = 2;

pub const NVENCAPI_VERSION: u32 = (NVENCAPI_MAJOR_VERSION << 4) | NVENCAPI_MINOR_VERSION;

pub inline fn NVENCAPI_STRUCT_VERSION(ver: u32, comptime T: type) u32 {
    return (NVENCAPI_VERSION << 24) | (ver << 16) | @as(u32, @sizeOf(T));
}

// Maximum input/output buffers
pub const NV_MAX_SEQ_HDR_LEN = 512;
pub const NV_ENC_NUM_MAX_BFRAMES = 16;
pub const NV_ENC_LOOKAHEAD_MAX_DEPTH = 32;

// ============================================================================
// NVENC Result Codes
// ============================================================================

pub const NVENCSTATUS = enum(i32) {
    NV_ENC_SUCCESS = 0,
    NV_ENC_ERR_NO_ENCODE_DEVICE = 1,
    NV_ENC_ERR_UNSUPPORTED_DEVICE = 2,
    NV_ENC_ERR_INVALID_ENCODERDEVICE = 3,
    NV_ENC_ERR_INVALID_DEVICE = 4,
    NV_ENC_ERR_DEVICE_NOT_EXIST = 5,
    NV_ENC_ERR_INVALID_PTR = 6,
    NV_ENC_ERR_INVALID_EVENT = 7,
    NV_ENC_ERR_INVALID_PARAM = 8,
    NV_ENC_ERR_INVALID_CALL = 9,
    NV_ENC_ERR_OUT_OF_MEMORY = 10,
    NV_ENC_ERR_ENCODER_NOT_INITIALIZED = 11,
    NV_ENC_ERR_UNSUPPORTED_PARAM = 12,
    NV_ENC_ERR_LOCK_BUSY = 13,
    NV_ENC_ERR_NOT_ENOUGH_BUFFER = 14,
    NV_ENC_ERR_INVALID_VERSION = 15,
    NV_ENC_ERR_MAP_FAILED = 16,
    NV_ENC_ERR_NEED_MORE_INPUT = 17,
    NV_ENC_ERR_ENCODER_BUSY = 18,
    NV_ENC_ERR_EVENT_NOT_REGISTERD = 19,
    NV_ENC_ERR_GENERIC = 20,
    NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY = 21,
    NV_ENC_ERR_UNIMPLEMENTED = 22,
    NV_ENC_ERR_RESOURCE_REGISTER_FAILED = 23,
    NV_ENC_ERR_RESOURCE_NOT_REGISTERED = 24,
    NV_ENC_ERR_RESOURCE_NOT_MAPPED = 25,
    _,

    pub fn isSuccess(self: NVENCSTATUS) bool {
        return self == .NV_ENC_SUCCESS;
    }

    pub fn description(self: NVENCSTATUS) []const u8 {
        return switch (self) {
            .NV_ENC_SUCCESS => "Success",
            .NV_ENC_ERR_NO_ENCODE_DEVICE => "No encode device available",
            .NV_ENC_ERR_UNSUPPORTED_DEVICE => "Unsupported device",
            .NV_ENC_ERR_INVALID_ENCODERDEVICE => "Invalid encoder device",
            .NV_ENC_ERR_INVALID_DEVICE => "Invalid device",
            .NV_ENC_ERR_DEVICE_NOT_EXIST => "Device does not exist",
            .NV_ENC_ERR_INVALID_PTR => "Invalid pointer",
            .NV_ENC_ERR_INVALID_EVENT => "Invalid event",
            .NV_ENC_ERR_INVALID_PARAM => "Invalid parameter",
            .NV_ENC_ERR_INVALID_CALL => "Invalid API call",
            .NV_ENC_ERR_OUT_OF_MEMORY => "Out of memory",
            .NV_ENC_ERR_ENCODER_NOT_INITIALIZED => "Encoder not initialized",
            .NV_ENC_ERR_UNSUPPORTED_PARAM => "Unsupported parameter",
            .NV_ENC_ERR_LOCK_BUSY => "Lock busy",
            .NV_ENC_ERR_NOT_ENOUGH_BUFFER => "Not enough buffer",
            .NV_ENC_ERR_INVALID_VERSION => "Invalid API version",
            .NV_ENC_ERR_MAP_FAILED => "Map failed",
            .NV_ENC_ERR_NEED_MORE_INPUT => "Need more input",
            .NV_ENC_ERR_ENCODER_BUSY => "Encoder busy",
            .NV_ENC_ERR_EVENT_NOT_REGISTERD => "Event not registered",
            .NV_ENC_ERR_GENERIC => "Generic error",
            .NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY => "Incompatible client key",
            .NV_ENC_ERR_UNIMPLEMENTED => "Unimplemented",
            .NV_ENC_ERR_RESOURCE_REGISTER_FAILED => "Resource register failed",
            .NV_ENC_ERR_RESOURCE_NOT_REGISTERED => "Resource not registered",
            .NV_ENC_ERR_RESOURCE_NOT_MAPPED => "Resource not mapped",
            else => "Unknown error",
        };
    }
};

// ============================================================================
// NVENC Codec GUIDs
// ============================================================================

pub const GUID = extern struct {
    Data1: u32,
    Data2: u16,
    Data3: u16,
    Data4: [8]u8,

    pub fn eql(self: GUID, other: GUID) bool {
        return self.Data1 == other.Data1 and
            self.Data2 == other.Data2 and
            self.Data3 == other.Data3 and
            std.mem.eql(u8, &self.Data4, &other.Data4);
    }
};

// Codec GUIDs
pub const NV_ENC_CODEC_H264_GUID = GUID{
    .Data1 = 0x6bc82762,
    .Data2 = 0x4e63,
    .Data3 = 0x4ca4,
    .Data4 = .{ 0xaa, 0x85, 0x1e, 0x50, 0xf3, 0x21, 0xf6, 0xbf },
};

pub const NV_ENC_CODEC_HEVC_GUID = GUID{
    .Data1 = 0x790cdc88,
    .Data2 = 0x4522,
    .Data3 = 0x4d7b,
    .Data4 = .{ 0x94, 0x25, 0xbd, 0xa9, 0x97, 0x5f, 0x76, 0x03 },
};

pub const NV_ENC_CODEC_AV1_GUID = GUID{
    .Data1 = 0x0a352289,
    .Data2 = 0x0aa7,
    .Data3 = 0x4759,
    .Data4 = .{ 0x86, 0x2d, 0x5d, 0x15, 0xcd, 0x16, 0xd2, 0x54 },
};

// Preset GUIDs (NVENC SDK 12.2)
pub const NV_ENC_PRESET_P1_GUID = GUID{
    .Data1 = 0xfc0a8d3e,
    .Data2 = 0x45f8,
    .Data3 = 0x4cf8,
    .Data4 = .{ 0x80, 0xc7, 0x29, 0x88, 0x71, 0x59, 0x0e, 0xbf },
};

pub const NV_ENC_PRESET_P2_GUID = GUID{
    .Data1 = 0xf581cfb8,
    .Data2 = 0xba3f,
    .Data3 = 0x4f53,
    .Data4 = .{ 0x85, 0xf0, 0x41, 0x85, 0xf8, 0x49, 0xf5, 0x6b },
};

pub const NV_ENC_PRESET_P3_GUID = GUID{
    .Data1 = 0x36850110,
    .Data2 = 0x3a07,
    .Data3 = 0x441f,
    .Data4 = .{ 0x94, 0xd5, 0x32, 0x70, 0xfe, 0x82, 0x06, 0x24 },
};

pub const NV_ENC_PRESET_P4_GUID = GUID{
    .Data1 = 0x90a7b826,
    .Data2 = 0xdf06,
    .Data3 = 0x4862,
    .Data4 = .{ 0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x8f, 0x81 },
};

pub const NV_ENC_PRESET_P5_GUID = GUID{
    .Data1 = 0x21c6e6b4,
    .Data2 = 0x297a,
    .Data3 = 0x4cba,
    .Data4 = .{ 0x99, 0x8f, 0xb6, 0xeb, 0xdb, 0x76, 0x76, 0x84 },
};

pub const NV_ENC_PRESET_P6_GUID = GUID{
    .Data1 = 0x8e75c279,
    .Data2 = 0x6299,
    .Data3 = 0x4ab6,
    .Data4 = .{ 0x83, 0x6a, 0x8c, 0xdd, 0x50, 0x31, 0xca, 0x12 },
};

pub const NV_ENC_PRESET_P7_GUID = GUID{
    .Data1 = 0x84848c12,
    .Data2 = 0x6f71,
    .Data3 = 0x4c13,
    .Data4 = .{ 0x93, 0x1b, 0x53, 0xe2, 0x83, 0xf5, 0x78, 0x53 },
};

// Profile GUIDs
pub const NV_ENC_H264_PROFILE_BASELINE_GUID = GUID{
    .Data1 = 0x0727bcaa,
    .Data2 = 0x78c4,
    .Data3 = 0x4c83,
    .Data4 = .{ 0x8c, 0x2f, 0xef, 0x3d, 0xff, 0x26, 0x7c, 0x6a },
};

pub const NV_ENC_H264_PROFILE_MAIN_GUID = GUID{
    .Data1 = 0x60b5c1d4,
    .Data2 = 0x67fe,
    .Data3 = 0x4790,
    .Data4 = .{ 0x94, 0xd5, 0xc4, 0x72, 0x6d, 0x7b, 0x6e, 0x6d },
};

pub const NV_ENC_H264_PROFILE_HIGH_GUID = GUID{
    .Data1 = 0xe7cbc309,
    .Data2 = 0x4f7a,
    .Data3 = 0x4b89,
    .Data4 = .{ 0xaf, 0x13, 0xa0, 0xcd, 0x29, 0x5e, 0x44, 0x8d },
};

pub const NV_ENC_HEVC_PROFILE_MAIN_GUID = GUID{
    .Data1 = 0xb514c39a,
    .Data2 = 0xb55b,
    .Data3 = 0x40fa,
    .Data4 = .{ 0x87, 0x8f, 0xf1, 0x25, 0x3b, 0x4d, 0xfd, 0xec },
};

pub const NV_ENC_HEVC_PROFILE_MAIN10_GUID = GUID{
    .Data1 = 0xfa4d2b6c,
    .Data2 = 0x3a5b,
    .Data3 = 0x411a,
    .Data4 = .{ 0x80, 0x18, 0x0a, 0x3f, 0x5e, 0x3c, 0x9b, 0xe5 },
};

pub const NV_ENC_AV1_PROFILE_MAIN_GUID = GUID{
    .Data1 = 0x5f2a39f5,
    .Data2 = 0xf14e,
    .Data3 = 0x4f95,
    .Data4 = .{ 0x9a, 0x9e, 0xb7, 0x66, 0x28, 0x6f, 0xf3, 0x6f },
};

// ============================================================================
// NVENC Enumerations
// ============================================================================

pub const NV_ENC_DEVICE_TYPE = enum(u32) {
    NV_ENC_DEVICE_TYPE_DIRECTX = 0,
    NV_ENC_DEVICE_TYPE_CUDA = 1,
    NV_ENC_DEVICE_TYPE_OPENGL = 2,
};

pub const NV_ENC_INPUT_RESOURCE_TYPE = enum(u32) {
    NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX = 0,
    NV_ENC_INPUT_RESOURCE_TYPE_CUDADEVICEPTR = 1,
    NV_ENC_INPUT_RESOURCE_TYPE_CUDAARRAY = 2,
    NV_ENC_INPUT_RESOURCE_TYPE_OPENGL_TEX = 3,
};

pub const NV_ENC_BUFFER_FORMAT = enum(u32) {
    NV_ENC_BUFFER_FORMAT_UNDEFINED = 0x00000000,
    NV_ENC_BUFFER_FORMAT_NV12 = 0x00000001,
    NV_ENC_BUFFER_FORMAT_YV12 = 0x00000010,
    NV_ENC_BUFFER_FORMAT_IYUV = 0x00000100,
    NV_ENC_BUFFER_FORMAT_YUV444 = 0x00001000,
    NV_ENC_BUFFER_FORMAT_YUV420_10BIT = 0x00010000,
    NV_ENC_BUFFER_FORMAT_YUV444_10BIT = 0x00100000,
    NV_ENC_BUFFER_FORMAT_ARGB = 0x01000000,
    NV_ENC_BUFFER_FORMAT_ARGB10 = 0x02000000,
    NV_ENC_BUFFER_FORMAT_AYUV = 0x04000000,
    NV_ENC_BUFFER_FORMAT_ABGR = 0x10000000,
    NV_ENC_BUFFER_FORMAT_ABGR10 = 0x20000000,
};

pub const NV_ENC_PIC_TYPE = enum(u32) {
    NV_ENC_PIC_TYPE_P = 0,
    NV_ENC_PIC_TYPE_B = 1,
    NV_ENC_PIC_TYPE_I = 2,
    NV_ENC_PIC_TYPE_IDR = 3,
    NV_ENC_PIC_TYPE_BI = 4,
    NV_ENC_PIC_TYPE_SKIPPED = 5,
    NV_ENC_PIC_TYPE_INTRA_REFRESH = 6,
    NV_ENC_PIC_TYPE_NONREF_P = 7,
    NV_ENC_PIC_TYPE_UNKNOWN = 0xFF,
};

pub const NV_ENC_PIC_STRUCT = enum(u32) {
    NV_ENC_PIC_STRUCT_FRAME = 0x01,
    NV_ENC_PIC_STRUCT_FIELD_TOP_BOTTOM = 0x02,
    NV_ENC_PIC_STRUCT_FIELD_BOTTOM_TOP = 0x03,
};

pub const NV_ENC_RC_MODE = enum(u32) {
    NV_ENC_PARAMS_RC_CONSTQP = 0x0,
    NV_ENC_PARAMS_RC_VBR = 0x1,
    NV_ENC_PARAMS_RC_CBR = 0x2,
    NV_ENC_PARAMS_RC_CBR_LOWDELAY_HQ = 0x8,
    NV_ENC_PARAMS_RC_CBR_HQ = 0x10,
    NV_ENC_PARAMS_RC_VBR_HQ = 0x20,
};

pub const NV_ENC_TUNING_INFO = enum(u32) {
    NV_ENC_TUNING_INFO_UNDEFINED = 0,
    NV_ENC_TUNING_INFO_HIGH_QUALITY = 1,
    NV_ENC_TUNING_INFO_LOW_LATENCY = 2,
    NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY = 3,
    NV_ENC_TUNING_INFO_LOSSLESS = 4,
};

pub const NV_ENC_MULTI_PASS = enum(u32) {
    NV_ENC_MULTI_PASS_DISABLED = 0,
    NV_ENC_TWO_PASS_QUARTER_RESOLUTION = 1,
    NV_ENC_TWO_PASS_FULL_RESOLUTION = 2,
};

// ============================================================================
// NVENC Structures
// ============================================================================

pub const NV_ENC_CAPS_PARAM = extern struct {
    version: u32,
    capsToQuery: u32,
    reserved: u32 = 0,
};

pub const NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS = extern struct {
    version: u32,
    deviceType: NV_ENC_DEVICE_TYPE,
    device: ?*anyopaque,
    reserved: ?*anyopaque = null,
    apiVersion: u32,
    reserved1: [253]u32 = [_]u32{0} ** 253,
    reserved2: [64]?*anyopaque = [_]?*anyopaque{null} ** 64,
};

pub const NV_ENC_PRESET_CONFIG = extern struct {
    version: u32,
    presetCfg: NV_ENC_CONFIG,
    reserved1: [255]u32 = [_]u32{0} ** 255,
    reserved2: [64]?*anyopaque = [_]?*anyopaque{null} ** 64,
};

pub const NV_ENC_CONFIG = extern struct {
    version: u32,
    profileGUID: GUID,
    gopLength: u32,
    frameIntervalP: i32,
    monoChromeEncoding: u32,
    frameFieldMode: u32,
    mvPrecision: u32,
    rcParams: NV_ENC_RC_PARAMS,
    encodeCodecConfig: NV_ENC_CODEC_CONFIG,
    reserved: [278]u32 = [_]u32{0} ** 278,
    reserved2: [64]?*anyopaque = [_]?*anyopaque{null} ** 64,
};

pub const NV_ENC_RC_PARAMS = extern struct {
    version: u32,
    rateControlMode: NV_ENC_RC_MODE,
    constQP: NV_ENC_QP,
    averageBitRate: u32,
    maxBitRate: u32,
    vbvBufferSize: u32,
    vbvInitialDelay: u32,
    enableMinQP: u32,
    enableMaxQP: u32,
    minQP: NV_ENC_QP,
    maxQP: NV_ENC_QP,
    enableInitialRCQP: u32,
    initialRCQP: NV_ENC_QP,
    enableAQ: u32,
    enableLookahead: u32,
    lookaheadDepth: u32,
    disableIadapt: u32,
    disableBadapt: u32,
    enableTemporalAQ: u32,
    zeroReorderDelay: u32,
    enableNonRefP: u32,
    strictGOPTarget: u32,
    aqStrength: u32,
    enableExtQPDeltaMap: u32,
    qpMapMode: u32,
    multiPass: NV_ENC_MULTI_PASS,
    alphaLayerBitrateRatio: u32,
    cbQPIndexOffset: i32,
    crQPIndexOffset: i32,
    reserved: [6]u32 = [_]u32{0} ** 6,
};

pub const NV_ENC_QP = extern struct {
    qpInterP: u32,
    qpInterB: u32,
    qpIntra: u32,
};

pub const NV_ENC_CODEC_CONFIG = extern union {
    h264Config: NV_ENC_CONFIG_H264,
    hevcConfig: NV_ENC_CONFIG_HEVC,
    av1Config: NV_ENC_CONFIG_AV1,
    reserved: [320]u32,
};

pub const NV_ENC_CONFIG_H264 = extern struct {
    enableStereoMVC: u32,
    hierarchicalPFrames: u32,
    hierarchicalBFrames: u32,
    outputBufferingPeriodSEI: u32,
    outputPictureTimingSEI: u32,
    outputAUD: u32,
    disableSPSPPS: u32,
    outputFramePackingSEI: u32,
    outputRecoveryPointSEI: u32,
    enableIntraRefresh: u32,
    enableConstrainedEncoding: u32,
    repeatSPSPPS: u32,
    enableVFR: u32,
    enableLTR: u32,
    qpPrimeYZeroTransformBypassFlag: u32,
    useConstrainedIntraPred: u32,
    enableFillerDataInsertion: u32,
    reserved: u32,
    level: u32,
    idrPeriod: u32,
    separateColourPlaneFlag: u32,
    disableDeblockingFilterIDC: u32,
    numTemporalLayers: u32,
    spsId: u32,
    ppsId: u32,
    adaptiveTransformMode: u32,
    fmoMode: u32,
    bdirectMode: u32,
    entropyCodingMode: u32,
    stereoMode: u32,
    intraRefreshPeriod: u32,
    intraRefreshCnt: u32,
    maxNumRefFrames: u32,
    sliceMode: u32,
    sliceModeData: u32,
    h264VUIParameters: NV_ENC_CONFIG_H264_VUI_PARAMETERS,
    ltrNumFrames: u32,
    ltrTrustMode: u32,
    chromaFormatIDC: u32,
    maxTemporalLayers: u32,
    useBFramesAsRef: u32,
    numRefL0: u32,
    numRefL1: u32,
    reserved1: [267]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_CONFIG_H264_VUI_PARAMETERS = extern struct {
    overscanInfoPresentFlag: u32,
    overscanInfo: u32,
    videoSignalTypePresentFlag: u32,
    videoFormat: u32,
    videoFullRangeFlag: u32,
    colourDescriptionPresentFlag: u32,
    colourPrimaries: u32,
    transferCharacteristics: u32,
    colourMatrix: u32,
    chromaSampleLocationFlag: u32,
    chromaSampleLocationTop: u32,
    chromaSampleLocationBot: u32,
    bitstreamRestrictionFlag: u32,
    reserved: [15]u32,
};

pub const NV_ENC_CONFIG_HEVC = extern struct {
    level: u32,
    tier: u32,
    minCUSize: u32,
    maxCUSize: u32,
    useConstrainedIntraPred: u32,
    disableDeblockAcrossSliceBoundary: u32,
    outputBufferingPeriodSEI: u32,
    outputPictureTimingSEI: u32,
    outputAUD: u32,
    enableLTR: u32,
    disableSPSPPS: u32,
    repeatSPSPPS: u32,
    enableIntraRefresh: u32,
    chromaFormatIDC: u32,
    pixelBitDepthMinus8: u32,
    enableFillerDataInsertion: u32,
    enableConstrainedEncoding: u32,
    enableAlphaLayerEncoding: u32,
    reserved: u32,
    idrPeriod: u32,
    intraRefreshPeriod: u32,
    intraRefreshCnt: u32,
    maxNumRefFramesInDPB: u32,
    ltrNumFrames: u32,
    vpsId: u32,
    spsId: u32,
    ppsId: u32,
    sliceMode: u32,
    sliceModeData: u32,
    maxTemporalLayersMinus1: u32,
    hevcVUIParameters: NV_ENC_CONFIG_HEVC_VUI_PARAMETERS,
    ltrTrustMode: u32,
    useBFramesAsRef: u32,
    numRefL0: u32,
    numRefL1: u32,
    reserved1: [214]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_CONFIG_HEVC_VUI_PARAMETERS = extern struct {
    overscanInfoPresentFlag: u32,
    overscanInfo: u32,
    videoSignalTypePresentFlag: u32,
    videoFormat: u32,
    videoFullRangeFlag: u32,
    colourDescriptionPresentFlag: u32,
    colourPrimaries: u32,
    transferCharacteristics: u32,
    colourMatrix: u32,
    chromaSampleLocationFlag: u32,
    chromaSampleLocationTop: u32,
    chromaSampleLocationBot: u32,
    bitstreamRestrictionFlag: u32,
    reserved: [15]u32,
};

pub const NV_ENC_CONFIG_AV1 = extern struct {
    level: u32,
    tier: u32,
    minPartSize: u32,
    maxPartSize: u32,
    outputAnnexBFormat: u32,
    enableBitstreamPadding: u32,
    enableCustomTileConfig: u32,
    enableFilmGrainParams: u32,
    inputPixelBitDepthMinus8: u32,
    pixelBitDepthMinus8: u32,
    idrPeriod: u32,
    intraRefreshPeriod: u32,
    intraRefreshCnt: u32,
    maxNumRefFramesInDPB: u32,
    numTileColumns: u32,
    numTileRows: u32,
    maxTemporalLayersMinus1: u32,
    colorPrimaries: u32,
    transferCharacteristics: u32,
    matrixCoefficients: u32,
    colorRange: u32,
    chromaSamplePosition: u32,
    useBFramesAsRef: u32,
    numFwdRefs: u32,
    numBwdRefs: u32,
    reserved1: [225]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_INITIALIZE_PARAMS = extern struct {
    version: u32,
    encodeGUID: GUID,
    presetGUID: GUID,
    encodeWidth: u32,
    encodeHeight: u32,
    darWidth: u32,
    darHeight: u32,
    frameRateNum: u32,
    frameRateDen: u32,
    enableEncodeAsync: u32,
    enablePTD: u32,
    reportSliceOffsets: u32,
    enableSubFrameWrite: u32,
    enableExternalMEHints: u32,
    enableMEOnlyMode: u32,
    enableWeightedPrediction: u32,
    splitEncodeMode: u32,
    outputDpbTableToApp: u32,
    reserved: u32,
    privDataSize: u32,
    privData: ?*anyopaque,
    encodeConfig: ?*NV_ENC_CONFIG,
    maxEncodeWidth: u32,
    maxEncodeHeight: u32,
    maxMEHintCountsPerBlock: [2]u32,
    tuningInfo: NV_ENC_TUNING_INFO,
    reserved1: [288]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_CREATE_INPUT_BUFFER = extern struct {
    version: u32,
    width: u32,
    height: u32,
    memoryHeap: u32,
    bufferFmt: NV_ENC_BUFFER_FORMAT,
    reserved: u32,
    inputBuffer: ?*anyopaque,
    pSysMemBuffer: ?*anyopaque,
    reserved1: [57]u32,
    reserved2: [63]?*anyopaque,
};

pub const NV_ENC_CREATE_BITSTREAM_BUFFER = extern struct {
    version: u32,
    size: u32,
    memoryHeap: u32,
    reserved: u32,
    bitstreamBuffer: ?*anyopaque,
    bitstreamBufferPtr: ?*anyopaque,
    reserved1: [58]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_LOCK_INPUT_BUFFER = extern struct {
    version: u32,
    reserved: u32,
    inputBuffer: ?*anyopaque,
    bufferDataPtr: ?*anyopaque,
    pitch: u32,
    reserved1: [59]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_LOCK_BITSTREAM = extern struct {
    version: u32,
    doNotWait: u32,
    ltrFrame: u32,
    getRCStats: u32,
    reservedBitFields: u32,
    frameAvgQP: u32,
    outputBitstream: ?*anyopaque,
    sliceOffsets: ?*u32,
    frameIdx: u32,
    hwEncodeStatus: u32,
    numSlices: u32,
    bitstreamSizeInBytes: u32,
    outputTimeStamp: u64,
    outputDuration: u64,
    bitstreamBufferPtr: ?*anyopaque,
    pictureType: NV_ENC_PIC_TYPE,
    pictureStruct: NV_ENC_PIC_STRUCT,
    frameIdxDisplay: u32,
    reserved: u32,
    intraMBCount: u32,
    interMBCount: u32,
    averageMVX: i32,
    averageMVY: i32,
    alphaLayerSizeInBytes: u32,
    ltrFrameIdx: u32,
    ltrFrameBitmap: u32,
    reserved1: [13]u32,
    reserved2: [64]?*anyopaque,
};

pub const NV_ENC_PIC_PARAMS = extern struct {
    version: u32,
    inputWidth: u32,
    inputHeight: u32,
    inputPitch: u32,
    encodePicFlags: u32,
    frameIdx: u32,
    inputTimeStamp: u64,
    inputDuration: u64,
    inputBuffer: ?*anyopaque,
    outputBitstream: ?*anyopaque,
    completionEvent: ?*anyopaque,
    bufferFmt: NV_ENC_BUFFER_FORMAT,
    pictureStruct: NV_ENC_PIC_STRUCT,
    pictureType: NV_ENC_PIC_TYPE,
    codecPicParams: NV_ENC_CODEC_PIC_PARAMS,
    meHintCountsPerBlock: [2]u32,
    meExternalHints: ?*anyopaque,
    reserved: u32,
    qpDeltaMapSize: u32,
    qpDeltaMap: ?*anyopaque,
    reserved3: [287]u32,
    reserved4: [59]?*anyopaque,
};

pub const NV_ENC_CODEC_PIC_PARAMS = extern union {
    h264PicParams: NV_ENC_PIC_PARAMS_H264,
    hevcPicParams: NV_ENC_PIC_PARAMS_HEVC,
    av1PicParams: NV_ENC_PIC_PARAMS_AV1,
    reserved: [256]u32,
};

pub const NV_ENC_PIC_PARAMS_H264 = extern struct {
    displayPOCSyntax: u32,
    reserved3: u32,
    refPicFlag: u32,
    colourPlaneId: u32,
    forceIntraRefreshWithFrameCnt: u32,
    constrainedFrame: u32,
    sliceModeDataUpdate: u32,
    ltrMarkFrame: u32,
    ltrUseFrames: u32,
    ltrMarkFrameIdx: u32,
    ltrUseFrameBitmap: u32,
    ltrUsageMode: u32,
    forceIntraSliceCount: u32,
    forceIntraSliceIdx: ?*u32,
    seiPayloadArray: ?*anyopaque,
    seiPayloadArrayCnt: u32,
    reserved: [221]u32,
    reserved2: [61]?*anyopaque,
};

pub const NV_ENC_PIC_PARAMS_HEVC = extern struct {
    displayPOCSyntax: u32,
    refPicFlag: u32,
    temporalId: u32,
    forceIntraRefreshWithFrameCnt: u32,
    constrainedFrame: u32,
    sliceModeDataUpdate: u32,
    ltrMarkFrame: u32,
    ltrUseFrames: u32,
    ltrMarkFrameIdx: u32,
    ltrUseFrameBitmap: u32,
    ltrUsageMode: u32,
    reserved: [223]u32,
    reserved2: [62]?*anyopaque,
};

pub const NV_ENC_PIC_PARAMS_AV1 = extern struct {
    reserved: u32,
    refPicFlag: u32,
    temporalId: u32,
    forceIntraRefreshWithFrameCnt: u32,
    constrainedFrame: u32,
    obuPayloadArrayCnt: u32,
    obuPayloadArray: ?*anyopaque,
    reserved1: [249]u32,
    reserved2: [62]?*anyopaque,
};

// ============================================================================
// NVENC Function Table
// ============================================================================

pub const NV_ENCODE_API_FUNCTION_LIST = extern struct {
    version: u32,
    reserved: u32,
    nvEncOpenEncodeSession: ?*anyopaque,
    nvEncGetEncodeGUIDCount: ?*anyopaque,
    nvEncGetEncodeGUIDs: ?*anyopaque,
    nvEncGetEncodeProfileGUIDCount: ?*anyopaque,
    nvEncGetEncodeProfileGUIDs: ?*anyopaque,
    nvEncGetInputFormatCount: ?*anyopaque,
    nvEncGetInputFormats: ?*anyopaque,
    nvEncGetEncodeCaps: ?*anyopaque,
    nvEncGetEncodePresetCount: ?*anyopaque,
    nvEncGetEncodePresetGUIDs: ?*anyopaque,
    nvEncGetEncodePresetConfig: ?*anyopaque,
    nvEncInitializeEncoder: ?*anyopaque,
    nvEncCreateInputBuffer: ?*anyopaque,
    nvEncDestroyInputBuffer: ?*anyopaque,
    nvEncCreateBitstreamBuffer: ?*anyopaque,
    nvEncDestroyBitstreamBuffer: ?*anyopaque,
    nvEncEncodePicture: ?*anyopaque,
    nvEncLockBitstream: ?*anyopaque,
    nvEncUnlockBitstream: ?*anyopaque,
    nvEncLockInputBuffer: ?*anyopaque,
    nvEncUnlockInputBuffer: ?*anyopaque,
    nvEncGetEncodeStats: ?*anyopaque,
    nvEncGetSequenceParams: ?*anyopaque,
    nvEncRegisterAsyncEvent: ?*anyopaque,
    nvEncUnregisterAsyncEvent: ?*anyopaque,
    nvEncMapInputResource: ?*anyopaque,
    nvEncUnmapInputResource: ?*anyopaque,
    nvEncDestroyEncoder: ?*anyopaque,
    nvEncInvalidateRefFrames: ?*anyopaque,
    nvEncOpenEncodeSessionEx: ?*anyopaque,
    nvEncRegisterResource: ?*anyopaque,
    nvEncUnregisterResource: ?*anyopaque,
    nvEncReconfigureEncoder: ?*anyopaque,
    reserved1: ?*anyopaque,
    nvEncCreateMVBuffer: ?*anyopaque,
    nvEncDestroyMVBuffer: ?*anyopaque,
    nvEncRunMotionEstimationOnly: ?*anyopaque,
    nvEncGetLastErrorString: ?*anyopaque,
    nvEncSetIOCudaStreams: ?*anyopaque,
    nvEncGetEncodePresetConfigEx: ?*anyopaque,
    nvEncGetSequenceParamEx: ?*anyopaque,
    nvEncRestoreEncoderState: ?*anyopaque,
    nvEncLookaheadPicture: ?*anyopaque,
    reserved2: [277]?*anyopaque,
};

// ============================================================================
// NVENC Loader
// ============================================================================

pub const NvencError = error{
    LibraryNotFound,
    SymbolNotFound,
    InitFailed,
    SessionCreateFailed,
    UnsupportedCodec,
    InvalidParameter,
    EncodeFailed,
    OutOfMemory,
};

/// Dynamic NVENC library loader
pub const NvencLoader = struct {
    lib_handle: ?*anyopaque = null,
    fn_list: NV_ENCODE_API_FUNCTION_LIST = undefined,
    loaded: bool = false,
    api_version: u32 = 0,

    const Self = @This();

    // C library function types
    const DlOpenFn = *const fn ([*:0]const u8, c_int) callconv(.C) ?*anyopaque;
    const DlSymFn = *const fn (?*anyopaque, [*:0]const u8) callconv(.C) ?*anyopaque;
    const DlCloseFn = *const fn (?*anyopaque) callconv(.C) c_int;
    const DlErrorFn = *const fn () callconv(.C) ?[*:0]const u8;

    // NVENC API entry point type
    const NvEncodeAPICreateInstanceFn = *const fn (*NV_ENCODE_API_FUNCTION_LIST) callconv(.C) NVENCSTATUS;
    const NvEncodeAPIGetMaxSupportedVersionFn = *const fn (*u32) callconv(.C) NVENCSTATUS;

    // dlopen flags
    const RTLD_NOW = 0x2;
    const RTLD_LOCAL = 0x0;

    // Library paths to try
    const lib_paths = [_][*:0]const u8{
        "libnvidia-encode.so.1",
        "libnvidia-encode.so",
        "/usr/lib/libnvidia-encode.so.1",
        "/usr/lib64/libnvidia-encode.so.1",
        "/usr/lib/x86_64-linux-gnu/libnvidia-encode.so.1",
    };

    /// Load the NVENC library
    pub fn load(self: *Self) NvencError!void {
        if (self.loaded) return;

        // Get C library functions
        const c_lib = std.DynLib.open("libc.so.6") catch {
            return NvencError.LibraryNotFound;
        };
        defer c_lib.close();

        const dlopen_fn = c_lib.lookup(DlOpenFn, "dlopen") orelse return NvencError.SymbolNotFound;
        const dlsym_fn = c_lib.lookup(DlSymFn, "dlsym") orelse return NvencError.SymbolNotFound;
        const dlclose_fn = c_lib.lookup(DlCloseFn, "dlclose") orelse return NvencError.SymbolNotFound;
        _ = dlclose_fn;

        // Try each library path
        var handle: ?*anyopaque = null;
        for (lib_paths) |path| {
            handle = dlopen_fn(path, RTLD_NOW | RTLD_LOCAL);
            if (handle != null) break;
        }

        if (handle == null) {
            return NvencError.LibraryNotFound;
        }

        self.lib_handle = handle;

        // Get NvEncodeAPIGetMaxSupportedVersion
        const get_version_fn = @as(
            ?NvEncodeAPIGetMaxSupportedVersionFn,
            @ptrCast(dlsym_fn(handle, "NvEncodeAPIGetMaxSupportedVersion")),
        );

        if (get_version_fn) |fn_ptr| {
            const status = fn_ptr(&self.api_version);
            if (!status.isSuccess()) {
                return NvencError.InitFailed;
            }
        }

        // Get NvEncodeAPICreateInstance
        const create_instance_fn = @as(
            ?NvEncodeAPICreateInstanceFn,
            @ptrCast(dlsym_fn(handle, "NvEncodeAPICreateInstance")),
        );

        if (create_instance_fn == null) {
            return NvencError.SymbolNotFound;
        }

        // Initialize function list
        self.fn_list = std.mem.zeroes(NV_ENCODE_API_FUNCTION_LIST);
        self.fn_list.version = NVENCAPI_STRUCT_VERSION(2, NV_ENCODE_API_FUNCTION_LIST);

        // Get all function pointers
        const status = create_instance_fn.?(&self.fn_list);
        if (!status.isSuccess()) {
            return NvencError.InitFailed;
        }

        self.loaded = true;
    }

    /// Unload the library
    pub fn unload(self: *Self) void {
        if (!self.loaded) return;

        if (self.lib_handle) |handle| {
            // Get dlclose
            if (std.DynLib.open("libc.so.6")) |c_lib| {
                defer c_lib.close();
                if (c_lib.lookup(DlCloseFn, "dlclose")) |dlclose_fn| {
                    _ = dlclose_fn(handle);
                }
            } else |_| {}
        }

        self.lib_handle = null;
        self.loaded = false;
    }

    /// Check if NVENC is available on the system
    pub fn isAvailable() bool {
        // Check if library file exists
        const paths = [_][]const u8{
            "/usr/lib/libnvidia-encode.so.1",
            "/usr/lib64/libnvidia-encode.so.1",
            "/usr/lib/x86_64-linux-gnu/libnvidia-encode.so.1",
        };

        for (paths) |path| {
            if (std.fs.cwd().access(path, .{})) |_| {
                return true;
            } else |_| {}
        }

        // Also check via ldconfig cache
        var result = std.process.Child.run(.{
            .allocator = std.heap.page_allocator,
            .argv = &[_][]const u8{ "ldconfig", "-p" },
        }) catch return false;
        defer {
            std.heap.page_allocator.free(result.stdout);
            std.heap.page_allocator.free(result.stderr);
        }

        return std.mem.indexOf(u8, result.stdout, "libnvidia-encode") != null;
    }

    /// Get the loaded API version
    pub fn getApiVersion(self: *const Self) ?struct { major: u32, minor: u32 } {
        if (!self.loaded) return null;
        return .{
            .major = (self.api_version >> 4) & 0xF,
            .minor = self.api_version & 0xF,
        };
    }

    /// Get function list (only valid after load)
    pub fn getFunctionList(self: *Self) ?*NV_ENCODE_API_FUNCTION_LIST {
        if (!self.loaded) return null;
        return &self.fn_list;
    }
};

// Global loader instance
var global_loader: NvencLoader = .{};

pub fn getLoader() *NvencLoader {
    return &global_loader;
}

// ============================================================================
// High-Level Encoder Wrapper
// ============================================================================

pub const NvencSession = struct {
    encoder: ?*anyopaque = null,
    config: NV_ENC_CONFIG = undefined,
    init_params: NV_ENC_INITIALIZE_PARAMS = undefined,
    initialized: bool = false,

    // Buffers
    input_buffers: [8]?*anyopaque = [_]?*anyopaque{null} ** 8,
    output_buffers: [8]?*anyopaque = [_]?*anyopaque{null} ** 8,
    buffer_count: usize = 0,
    current_buffer: usize = 0,

    const Self = @This();

    pub fn create(cuda_context: ?*anyopaque, codec: GUID, width: u32, height: u32, framerate: u32) NvencError!Self {
        var session = Self{};

        // TODO: Actual NVENC session creation
        // 1. NvEncOpenEncodeSessionEx
        // 2. NvEncGetEncodePresetConfigEx
        // 3. NvEncInitializeEncoder
        // 4. Create input/output buffers

        _ = cuda_context;

        session.init_params = std.mem.zeroes(NV_ENC_INITIALIZE_PARAMS);
        session.init_params.version = NVENCAPI_STRUCT_VERSION(2, NV_ENC_INITIALIZE_PARAMS);
        session.init_params.encodeGUID = codec;
        session.init_params.presetGUID = NV_ENC_PRESET_P4_GUID;
        session.init_params.encodeWidth = width;
        session.init_params.encodeHeight = height;
        session.init_params.frameRateNum = framerate;
        session.init_params.frameRateDen = 1;

        session.initialized = true;
        return session;
    }

    pub fn destroy(self: *Self) void {
        if (!self.initialized) return;

        // TODO: Cleanup
        // 1. Destroy buffers
        // 2. NvEncDestroyEncoder

        self.initialized = false;
    }

    pub fn encode(self: *Self, frame_data: []const u8, pts: i64) NvencError!?[]const u8 {
        if (!self.initialized) return NvencError.InitFailed;

        // TODO: Actual encoding
        // 1. Lock input buffer
        // 2. Copy frame data
        // 3. Unlock input buffer
        // 4. NvEncEncodePicture
        // 5. Lock bitstream
        // 6. Return encoded data

        _ = frame_data;
        _ = pts;

        return null;
    }

    pub fn flush(self: *Self) NvencError!?[]const u8 {
        if (!self.initialized) return NvencError.InitFailed;

        // TODO: Send EOS and drain
        return null;
    }
};

// ============================================================================
// Tests
// ============================================================================

test "nvenc status description" {
    try std.testing.expectEqualStrings("Success", NVENCSTATUS.NV_ENC_SUCCESS.description());
    try std.testing.expectEqualStrings("Out of memory", NVENCSTATUS.NV_ENC_ERR_OUT_OF_MEMORY.description());
}

test "guid equality" {
    try std.testing.expect(NV_ENC_CODEC_H264_GUID.eql(NV_ENC_CODEC_H264_GUID));
    try std.testing.expect(!NV_ENC_CODEC_H264_GUID.eql(NV_ENC_CODEC_HEVC_GUID));
}

test "struct version" {
    const struct_ver = NVENCAPI_STRUCT_VERSION(2, NV_ENC_INITIALIZE_PARAMS);
    try std.testing.expect(struct_ver > 0);
}

test "nvenc loader" {
    try std.testing.expect(NvencLoader.isAvailable());
}
