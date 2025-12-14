//! Capture stream utilities

use crate::types::Frame;
use std::sync::mpsc;

/// A stream of captured frames
pub struct CaptureStream {
    receiver: mpsc::Receiver<Frame>,
}

impl CaptureStream {
    /// Create a new capture stream with sender/receiver pair
    pub fn new() -> (CaptureStreamSender, Self) {
        let (sender, receiver) = mpsc::channel();
        (CaptureStreamSender { sender }, Self { receiver })
    }

    /// Try to receive the next frame without blocking
    pub fn try_recv(&self) -> Option<Frame> {
        self.receiver.try_recv().ok()
    }

    /// Receive the next frame, blocking until available
    pub fn recv(&self) -> Option<Frame> {
        self.receiver.recv().ok()
    }

    /// Receive with timeout
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Option<Frame> {
        self.receiver.recv_timeout(timeout).ok()
    }
}

impl Default for CaptureStream {
    fn default() -> Self {
        let (_, stream) = Self::new();
        stream
    }
}

/// Sender side of capture stream
pub struct CaptureStreamSender {
    sender: mpsc::Sender<Frame>,
}

impl CaptureStreamSender {
    /// Send a frame
    pub fn send(&self, frame: Frame) -> Result<(), Frame> {
        self.sender.send(frame).map_err(|e| e.0)
    }
}

impl Clone for CaptureStreamSender {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
