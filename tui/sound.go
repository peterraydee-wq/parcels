package main

// C5 E5 G5 up for success, a low buzz for failure.
var (
	toneOK   = [][2]int{{523, 70}, {659, 70}, {784, 120}}
	toneFail = [][2]int{{196, 160}, {147, 260}}
	toneBoot = [][2]int{{392, 50}, {523, 50}}
)
