"""Safe self-update: find a newer signed release, download it, verify it, swap the exe."""


class UpdateError(Exception):
    """Anything that makes an update untrustworthy or impossible; the message is shown to the player."""
