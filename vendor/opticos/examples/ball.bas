' Bouncing ball: SPACE stops it
DEF PROC move
  PRINT AT y%, x%; " ";
  x% = x% + dx%: y% = y% + dy%
  IF x% <= 0 OR x% >= 63 THEN dx% = -dx%: BEEP 0.01, 24
  IF y% <= 1 OR y% >= 22 THEN dy% = -dy%: BEEP 0.01, 12
  PRINT AT y%, x%; "O";
ENDPROC
BORDER 1: PAPER 0: INK 5: CLS
PRINT AT 0, 0; "Bouncing ball - SPACE to stop"
x% = 10: y% = 5: dx% = 1: dy% = 1
FOR n% = 1 TO 300
  move
  WAIT 1
  IF KEY(" ") THEN EXIT FOR
NEXT
PRINT AT 23, 0; "Moves: "; n% - 1;
