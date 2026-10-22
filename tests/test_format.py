from datetime import UTC, datetime

import pytest

from maplelens.market.format import format_ago, format_mesos


@pytest.mark.parametrize(
    ("amount", "expected"),
    [
        (None, "?"),
        (6, "6"),
        (9.5, "9.5"),
        (119.25, "119"),
        (999, "999"),
        (1000, "1k"),
        (1320, "1.32k"),
        (10500, "10.5k"),
        (105000, "105k"),
        (150000, "150k"),
        (145000, "145k"),
        (159999, "160k"),
        (80000, "80k"),
        (250000, "250k"),
        (199999, "200k"),
        (100000, "100k"),
        (150_000_000, "150m"),
        (250_000_000_000, "250b"),
        (999_999, "1m"),
        (2_300_000, "2.3m"),
        (1.5e9, "1.5b"),
    ],
)
def test_format_mesos(amount, expected):
    assert format_mesos(amount) == expected


NOW = datetime(2026, 10, 22, 12, 0, tzinfo=UTC)


@pytest.mark.parametrize(
    ("iso", "expected"),
    [
        (None, "—"),
        ("2026-10-22T11:59:30Z", "just now"),
        ("2026-10-22T11:15:00Z", "45m ago"),
        ("2026-10-22T09:00:00Z", "3h ago"),
        ("2026-10-19T12:00:00Z", "3d ago"),
    ],
)
def test_format_ago(iso, expected):
    assert format_ago(iso, now=NOW) == expected
