' A recursive tree
DEF PROC branch(x, y, a, l)
  LOCAL x2, y2
  IF l < 4 THEN ENDPROC
  x2 = x + l * COS a: y2 = y + l * SIN a
  PLOT x, y: DRAW TO x2, y2
  branch x2, y2, a + 0.45, l * 0.7
  branch x2, y2, a - 0.45, l * 0.7
ENDPROC
BORDER 0: PAPER 0: INK 4: CLS
t = FRAMES
branch 128, 0, PI / 2, 50
INK 6: PRINT AT 0, 0; "A tree in "; INT((FRAMES - t) / 50); " seconds"
