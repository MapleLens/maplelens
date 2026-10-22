"""What the classicrank.com API says about one character."""

from dataclasses import dataclass
from typing import Any

# The only fields the card uses; asking for less keeps responses tiny.
API_FIELDS = "name,level,job,fame,overall_rank,job_rank,fame_rank,botting,last_checked,profile_url"


@dataclass(frozen=True)
class Rank:
    name: str
    level: int | None
    job: str | None
    fame: int | None
    overall_rank: int | None   # by level; for botters it is the place in the Botters ranking
    job_rank: int | None
    fame_rank: int | None
    botting: bool
    last_checked: str | None   # ISO-8601 UTC
    profile_url: str | None

    @classmethod
    def from_api(cls, raw: dict[str, Any]) -> "Rank":
        """Parse one character object; unknown fields are ignored (the API may add some)."""
        return cls(
            name=str(raw["name"]),
            level=raw.get("level"),
            job=raw.get("job"),
            fame=raw.get("fame"),
            overall_rank=raw.get("overall_rank"),
            job_rank=raw.get("job_rank"),
            fame_rank=raw.get("fame_rank"),
            botting=bool(raw.get("botting")),
            last_checked=raw.get("last_checked"),
            profile_url=raw.get("profile_url"),
        )
