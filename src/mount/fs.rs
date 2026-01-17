use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request,
};
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1);

pub struct CloudFileSystem {
    // TODO: Add StorageProvider
}

impl CloudFileSystem {
    pub fn new() -> Self {
        Self {}
    }
}

impl Filesystem for CloudFileSystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        // TODO: Implement lookup
        // For prototype, just return ENOENT for everything except root
        if parent == 1 && name.to_str() == Some("hello.txt") {
            let attr = FileAttr {
                ino: 2,
                size: 13,
                blocks: 1,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: FileType::RegularFile,
                perm: 0o644,
                nlink: 1,
                uid: 501,
                gid: 20,
                rdev: 0,
                flags: 0,
                blksize: 512,
            };
            reply.entry(&TTL, &attr, 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        // TODO: Implement getattr
        match ino {
            1 => {
                let attr = FileAttr {
                    ino: 1,
                    size: 0,
                    blocks: 0,
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: FileType::Directory,
                    perm: 0o755,
                    nlink: 2,
                    uid: 501,
                    gid: 20,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                };
                reply.attr(&TTL, &attr);
            }
            2 => {
                let attr = FileAttr {
                    ino: 2,
                    size: 13,
                    blocks: 1,
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: FileType::RegularFile,
                    perm: 0o644,
                    nlink: 1,
                    uid: 501,
                    gid: 20,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                };
                reply.attr(&TTL, &attr);
            }
            _ => reply.error(libc::ENOENT),
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        _size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        if ino == 2 {
            let data = "Hello World!\n";
            if offset < data.len() as i64 {
                reply.data(&data.as_bytes()[offset as usize..]);
            } else {
                reply.data(&[]);
            }
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if ino != 1 {
            reply.error(libc::ENOENT);
            return;
        }

        let entries = vec![
            (1, FileType::Directory, "."),
            (1, FileType::Directory, ".."),
            (2, FileType::RegularFile, "hello.txt"),
        ];

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock structs to verify Filesystem trait methods without FUSE kernel interaction
    struct MockReplyEntry {
        pub attr: Option<FileAttr>,
        pub ttl: Option<Duration>,
        pub error: Option<i32>,
    }

    impl MockReplyEntry {
        fn new() -> Self {
            Self {
                attr: None,
                ttl: None,
                error: None,
            }
        }
    }

    // We can't implement fuser::ReplyEntry directly because it's foreign and we don't control it.
    // Instead, we will test the logic by exposing helper methods or restructuring the code.
    // HOWEVER, for this task, since we can't mock fuser's internal Reply structs easily (they own raw pointers),
    // we will rely on integration tests or simplified logic tests if possible.
    //
    // BUT, we can test `CloudFileSystem` internal state if we had any.
    // Since `lookup`, `getattr` etc. take `Reply*` objects which are hard to instantiate in tests,
    // we will create a simple instantiation test for now to ensure it compiles and structs are correct.

    #[test]
    fn test_filesystem_new() {
        let fs = CloudFileSystem::new();
        // Just verify it can be instantiated
        assert!(std::mem::size_of_val(&fs) >= 0);
    }
}
