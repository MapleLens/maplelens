"""What the map card shows, as plain data (pure)."""

from dataclasses import dataclass


@dataclass(frozen=True)
class Mob:
    id: int
    name: str
    level: int
    exp: int
    hp: int
    count: int   # spawn points of this monster on the map


@dataclass(frozen=True)
class Npc:
    id: int
    name: str
    role: str   # "Bowman Instructor", "" when it only talks


@dataclass(frozen=True)
class MapInfo:
    id: int
    name: str
    street: str
    is_town: bool
    mobs: tuple[Mob, ...]   # most spawns first
    npcs: tuple[Npc, ...]

    @property
    def spawns(self) -> int:
        return sum(m.count for m in self.mobs)

    @property
    def levels(self) -> tuple[int, int] | None:
        return (min(m.level for m in self.mobs), max(m.level for m in self.mobs)) if self.mobs else None


@dataclass(frozen=True)
class Location:
    """A world-map tooltip title, "Victoria Road : Henesys"."""

    street: str
    name: str
