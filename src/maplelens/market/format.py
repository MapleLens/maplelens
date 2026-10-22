"""Human-friendly number and time formatting for overlay text."""

from datetime import UTC, datetime


def _trim(value: float, unit: str) -> str:
    decimals = 2 if value < 10 else 1 if value < 100 else 0
    text = f"{value:.{decimals}f}"
    # Only fractional zeros are padding; whole-number zeros carry value.
    if decimals:
        text = text.rstrip("0").rstrip(".")
    return text + unit


def format_mesos(amount: float | None) -> str:
    """1320 -> '1.32k', 10500 -> '10.5k', 2_300_000 -> '2.3m'. Under 1000 stays plain."""
    if amount is None:
        return "?"
    if amount < 1000:
        return f"{amount:.0f}" if amount >= 100 else f"{amount:.2f}".rstrip("0").rstrip(".")
    for divisor, unit in ((1e3, "k"), (1e6, "m"), (1e9, "b")):
        scaled = amount / divisor
        if round(scaled, 2) < 1000 or unit == "b":
            return _trim(scaled, unit)
    raise AssertionError("unreachable")


def format_ago(iso_utc: str | None, now: datetime | None = None) -> str:
    """'3h ago' style age of an ISO-8601 timestamp ('—' when unknown)."""
    if not iso_utc:
        return "—"
    now = now or datetime.now(UTC)
    seconds = (now - datetime.fromisoformat(iso_utc.replace("Z", "+00:00"))).total_seconds()
    if seconds < 90:
        return "just now"
    if seconds < 5400:
        return f"{seconds / 60:.0f}m ago"
    if seconds < 172800:
        return f"{seconds / 3600:.0f}h ago"
    return f"{seconds / 86400:.0f}d ago"
