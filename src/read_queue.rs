use std::{
    io::{self, BufRead, Read},
    mem,
    sync::mpsc::{self, Receiver, Sender},
};

// Thanks to tiny-http to come up with this "trampoline" idea, solved the
// pipelining problem pretty well:
// https://github.com/tiny-http/tiny-http/blob/master/src/util/sequential.rs
pub enum ReadQueue<R> {
    Head(R),
    Next(Receiver<R>),
}

pub struct QueuedReader<R>
where
    R: Read + Send,
{
    reader: Option<QueuedReaderInner<R>>,
    next: Sender<R>,
}

enum QueuedReaderInner<R> {
    Current(R),
    Waiting(Receiver<R>),
}

impl<R: Read + Send> ReadQueue<R> {
    pub fn new(reader: R) -> ReadQueue<R> {
        ReadQueue::Head(reader)
    }

    pub fn enqueue(&mut self) -> QueuedReader<R> {
        let (tx, rx) = mpsc::channel();

        match mem::replace(self, ReadQueue::Next(rx)) {
            ReadQueue::Head(reader) => QueuedReader {
                reader: Some(QueuedReaderInner::Current(reader)),
                next: tx,
            },
            ReadQueue::Next(previous) => QueuedReader {
                reader: Some(QueuedReaderInner::Waiting(previous)),
                next: tx,
            },
        }
    }
}

impl<R: Read + Send> Read for QueuedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.reader.as_mut().unwrap() {
            QueuedReaderInner::Current(ref mut reader) => reader.read(buf),
            QueuedReaderInner::Waiting(ref mut rx) => {
                let mut reader = rx.recv().unwrap();
                let result = reader.read(buf);
                self.reader = Some(QueuedReaderInner::Current(reader));
                result
            }
        }
    }
}

impl<R: BufRead + Send> BufRead for QueuedReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self.reader {
            Some(QueuedReaderInner::Current(ref mut reader)) => reader.fill_buf(),
            Some(QueuedReaderInner::Waiting(ref mut rx)) => {
                let reader = rx.recv().unwrap();
                self.reader = Some(QueuedReaderInner::Current(reader));
                self.fill_buf()
            }
            None => unreachable!(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self.reader {
            Some(QueuedReaderInner::Current(ref mut reader)) => reader.consume(amt),
            Some(QueuedReaderInner::Waiting(ref mut rx)) => {
                let reader = rx.recv().unwrap();
                self.reader = Some(QueuedReaderInner::Current(reader));
                self.consume(amt)
            }
            None => unreachable!(),
        }
    }
}

impl<R: Read + Send> Drop for QueuedReader<R> {
    #[allow(unused_must_use)]
    fn drop(&mut self) {
        match self.reader.take() {
            Some(QueuedReaderInner::Current(reader)) => {
                self.next.send(reader);
            }
            Some(QueuedReaderInner::Waiting(rx)) => {
                self.next.send(rx.recv().unwrap());
            }
            None => {}
        }
    }
}
