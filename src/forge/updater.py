"""
Atomic binary update utilities for FORGE.

Provides safe, atomic binary replacement using os.rename().
This ensures updates are applied completely or not at all,
preventing partial updates that could corrupt the binary.
"""

import os
import sys
import shutil
import hashlib
import tempfile
from pathlib import Path
from typing import Optional
import urllib.request
import logging

logger = logging.getLogger(__name__)


class BinaryUpdateError(Exception):
    """Raised when binary update fails."""
    pass


class BinaryUpdater:
    """
    Handles atomic binary updates using rename().

    On POSIX systems, os.rename() is atomic, ensuring that:
    1. Either the entire update succeeds, or
    2. The old binary remains intact

    This prevents partial updates and corrupted binaries.
    """

    def __init__(self, binary_path: Path | str):
        """
        Initialize the updater.

        Args:
            binary_path: Path to the current binary (being replaced)
        """
        self.binary_path = Path(binary_path)
        self.backup_path = self.binary_path.with_suffix(".bak")

    def _calculate_checksum(self, file_path: Path) -> str:
        """Calculate SHA256 checksum of a file."""
        sha256 = hashlib.sha256()
        with open(file_path, "rb") as f:
            for chunk in iter(lambda: f.read(8192), b""):
                sha256.update(chunk)
        return sha256.hexdigest()

    def verify_binary(self, new_binary_path: Path) -> bool:
        """
        Verify the new binary is valid.

        Args:
            new_binary_path: Path to the new binary to verify

        Returns:
            True if binary appears valid
        """
        # Basic checks
        if not new_binary_path.exists():
            logger.error(f"New binary not found: {new_binary_path}")
            return False

        if new_binary_path.stat().st_size < 1024:
            logger.error(f"New binary too small: {new_binary_path.stat().st_size} bytes")
            return False

        # Check if executable (Unix)
        if sys.platform != "win32":
            if not os.access(new_binary_path, os.X_OK):
                logger.warning(f"New binary not executable: {new_binary_path}")

        return True

    def create_backup(self) -> bool:
        """
        Create a backup of the current binary.

        Returns:
            True if backup created successfully
        """
        if not self.binary_path.exists():
            logger.warning(f"Current binary not found: {self.binary_path}")
            return True  # No backup needed for new install

        try:
            shutil.copy2(self.binary_path, self.backup_path)
            logger.info(f"Backup created: {self.backup_path}")
            return True
        except Exception as e:
            logger.error(f"Failed to create backup: {e}")
            return False

    def restore_backup(self) -> bool:
        """
        Restore from backup if update failed.

        Returns:
            True if restore succeeded
        """
        if not self.backup_path.exists():
            logger.error("No backup found to restore")
            return False

        try:
            shutil.copy2(self.backup_path, self.binary_path)
            logger.info(f"Backup restored: {self.binary_path}")
            return True
        except Exception as e:
            logger.error(f"Failed to restore backup: {e}")
            return False

    def atomic_update(self, new_binary_path: Path | str, verify: bool = True) -> bool:
        """
        Perform atomic update using os.rename().

        This is the core operation that ensures atomicity.
        On POSIX systems, rename() is atomic at the filesystem level.

        Args:
            new_binary_path: Path to the new binary file
            verify: Whether to verify the new binary before updating

        Returns:
            True if update succeeded
        """
        new_binary_path = Path(new_binary_path)

        # Verify new binary
        if verify and not self.verify_binary(new_binary_path):
            raise BinaryUpdateError("New binary verification failed")

        # Create backup of current binary
        if not self.create_backup():
            raise BinaryUpdateError("Failed to create backup")

        try:
            # Atomic rename operation
            # On POSIX: atomic, replaces target if exists
            # On Windows: may fail if target exists (need to delete first)
            if sys.platform == "win32":
                # Windows requires target to not exist
                if self.binary_path.exists():
                    self.binary_path.unlink()
                new_binary_path.rename(self.binary_path)
            else:
                # POSIX: atomic rename with replacement
                new_binary_path.rename(self.binary_path)

            logger.info(f"Atomic update complete: {self.binary_path}")

            # Verify update succeeded
            if not self.binary_path.exists():
                raise BinaryUpdateError("Binary disappeared after update")

            # Clean up backup on success
            if self.backup_path.exists():
                self.backup_path.unlink()

            return True

        except Exception as e:
            logger.error(f"Update failed: {e}")

            # Attempt to restore from backup
            if self.restore_backup():
                logger.info("Restored from backup after failed update")
            else:
                logger.error("Failed to restore from backup")

            raise BinaryUpdateError(f"Atomic update failed: {e}")

    def download_and_update(
        self,
        url: str,
        expected_checksum: Optional[str] = None,
        verify: bool = True,
    ) -> bool:
        """
        Download new binary and perform atomic update.

        Args:
            url: URL to download new binary from
            expected_checksum: Optional SHA256 checksum for verification
            verify: Whether to verify downloaded binary

        Returns:
            True if download and update succeeded
        """
        with tempfile.NamedTemporaryFile(delete=False, suffix=".bin") as tmp_file:
            tmp_path = Path(tmp_file.name)

        try:
            # Download to temporary file
            logger.info(f"Downloading from {url}...")
            urllib.request.urlretrieve(url, tmp_path)

            # Verify checksum if provided
            if expected_checksum:
                actual_checksum = self._calculate_checksum(tmp_path)
                if actual_checksum != expected_checksum:
                    raise BinaryUpdateError(
                        f"Checksum mismatch: expected {expected_checksum}, got {actual_checksum}"
                    )

            # Perform atomic update
            return self.atomic_update(tmp_path, verify=verify)

        except Exception as e:
            logger.error(f"Download and update failed: {e}")
            if tmp_path.exists():
                tmp_path.unlink()
            raise BinaryUpdateError(f"Download failed: {e}")


def atomic_symlink_update(target: Path | str, link_path: Path | str) -> bool:
    """
    Create or update a symlink atomically.

    Useful for managing active/rollback versions of binaries.

    Args:
        target: Path the symlink should point to
        link_path: Path where the symlink should be created

    Returns:
        True if symlink created/updated successfully
    """
    target = Path(target)
    link_path = Path(link_path)

    # Create temporary symlink
    temp_link = link_path.with_suffix(".tmp")

    try:
        # Create temporary symlink
        temp_link.symlink_to(target)

        # Atomic rename to final location
        temp_link.rename(link_path)

        logger.info(f"Symlink updated: {link_path} -> {target}")
        return True

    except Exception as e:
        logger.error(f"Symlink update failed: {e}")
        if temp_link.exists():
            temp_link.unlink()
        return False


if __name__ == "__main__":
    # Example usage
    import argparse

    parser = argparse.ArgumentParser(description="FORGE Binary Updater")
    parser.add_argument("new_binary", help="Path to new binary file")
    parser.add_argument(
        "--target",
        default=sys.executable,
        help="Target binary path to update",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="Skip binary verification",
    )

    args = parser.parse_args()

    updater = BinaryUpdater(args.target)
    try:
        success = updater.atomic_update(
            Path(args.new_binary),
            verify=not args.no_verify,
        )
        if success:
            print("Update successful!")
            sys.exit(0)
        else:
            print("Update failed!")
            sys.exit(1)
    except BinaryUpdateError as e:
        print(f"Update error: {e}")
        sys.exit(1)
