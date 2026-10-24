from maplelens.geometry import Box, UiScale

CLIENT = Box(0, 0, 2560, 1440)


def test_follows_window_height_until_measured():
    ui = UiScale()
    assert ui.of(CLIENT) == 1.2
    ui.set(CLIENT, 0.62)   # "Ideal Ratio": the UI keeps its size whatever the window
    assert ui.of(CLIENT) == 0.62


def test_measurement_is_dropped_when_the_window_is_resized():
    ui = UiScale()
    ui.set(CLIENT, 0.62)
    assert ui.of(Box(0, 0, 1920, 1200)) == 1.0
