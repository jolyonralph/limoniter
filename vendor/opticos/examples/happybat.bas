' =============================================================
' HAPPY BAT - a Flappy Bird-style game for OpticOS v1.0.1
' OpticBASIC runs the game: title, pipes and their gaps, score,
' best score, game over and restart. A machine-code block
' "hbcode" (60000-61094, source happybat.asm) does the work that
' touches thousands of screen bytes each step: scrolling the
' ground, moving and drawing the pipes, and moving, drawing and
' testing the bat, with gravity and the flap key.
'
' Machine code, called with CALL / USR. They take no register
' arguments, so BASIC POKEs what the code needs first:
'   CALL 60000      new playfield: clear, colour, draw ground;
'                   the bat starts at line PEEK 60009, at rest
'   r% = USR 60003  one step: 0 = flying, 1 = crashed, 2 = a
'                   pipe got past the bat (a point)
'   CALL 60006      colour the bat red (game over)
' Bytes BASIC POKEs:
'   60009 bat's top line (0-152)   60010 gap top  60011 gap end
'   60012 1 = start a new pipe     60013 1 = play, 0 = title
'   60014-60015 "HB": the code is loaded
'   60016 gravity  60017 flap speed  60018 fastest fall
'   60019 frames per step (speeds are in quarter pixels a step)
' =============================================================
' Main program. CLEAR 59999 keeps BASIC below 60000, then the
' code is loaded from tape unless it is already in memory.
CLEAR 59999
IF PEEK(60014) <> 72 OR PEEK(60015) <> 66 THEN LOAD "hbcode" CODE
RANDOMIZE
best% = 0
' the game-over box's rows, made once so no string is left over
box$ = STRING$(28, " ")
title
REPEAT
  play
  game_over
UNTIL FALSE
'
DEF PROC start
  ' The tape saves the program with AUTO start, so this runs
  ' once it has loaded. RUN begins again at the top, where the
  ' main program can use CLEAR (it must not be in a PROC).
  RUN
ENDPROC
'
DEF PROC header
  ' The score line along the top: name, score and best score.
  PRINT AT 0, 2; PAPER 1; INK 6; BRIGHT 1; "HAPPY BAT";
  PRINT AT 0, 24; PAPER 1; INK 7; BRIGHT 1; "SCORE "; score%; "   ";
  PRINT AT 0, 46; PAPER 1; INK 7; BRIGHT 1; "BEST "; best%; "   ";
ENDPROC
'
DEF PROC footer
  ' The help text on the bottom row, black on the green grass:
  ' on the title and game-over screens only (CALL 60000 clears
  ' it when a game starts).
  PRINT AT 23, 2; PAPER 4; INK 0; BRIGHT 1; "SPACE  flap"; TAB(44); "BREAK  stop";
ENDPROC
'
DEF PROC release
  ' Wait until SPACE is let go, so one press is not read twice.
  WHILE KEY(" ")
  WEND
ENDPROC
'
DEF PROC title
  ' The title screen. CALL 60000 then POKE 60013, 0 is title
  ' mode: only the ground scrolls, and BASIC bobs the bat up
  ' and down on the spot until SPACE is pressed.
  LOCAL t%, r%
  BORDER 0: PAPER 1: INK 7: BRIGHT 1: CLS
  score% = 0
  CALL 60000
  POKE 60013, 0
  header
  footer
  PRINT AT 5, 28; PAPER 8; INK 6; BRIGHT 1; "H A P P Y    B A T";
  PRINT AT 8, 26; PAPER 8; INK 7; BRIGHT 1; "Fly the bat through the gaps";
  PRINT AT 9, 26; PAPER 8; INK 7; BRIGHT 1; "in the pipes. Do not touch a";
  PRINT AT 10, 26; PAPER 8; INK 7; BRIGHT 1; "pipe, the ground or the sky!";
  PRINT AT 13, 26; PAPER 8; INK 5; BRIGHT 1; "SPACE    flap your wings";
  PRINT AT 14, 26; PAPER 8; INK 5; BRIGHT 1; "Score a point for each pipe.";
  PRINT AT 18, 28; PAPER 8; INK 6; BRIGHT 1; FLASH 1; "Press SPACE to start";
  t% = 0
  REPEAT
    t% = t% + 1
    POKE 60009, 60 + ABS(t% MOD 16 - 8)
    r% = USR 60003
  UNTIL KEY(" ")
  release
ENDPROC
'
DEF PROC play
  ' One game. The flight itself (SPACE, gravity, the top and
  ' the ground) is in the machine code, so that no press is
  ' missed; BASIC sets its constants here. Each pass of the
  ' loop is one step of 2 frames (25 a second): USR 60003
  ' moves everything 4 pixels and reports a crash or a point.
  ' BASIC starts a new pipe every n% steps: 32 (128 pixels
  ' apart) at first, closing up to 24 (96 pixels) as the score
  ' rises (see new_pipe). The bottom row is left empty in play.
  LOCAL n%, r%, g%, gap%
  POKE 60016, 2: ' gravity
  POKE 60017, 18: ' flap
  POKE 60018, 20: ' fastest fall
  POKE 60019, 2: ' frames per step
  POKE 60009, 64: ' the bat starts in the middle
  CALL 60000
  score% = 0
  header
  n% = 24: g% = 56: ' the first pipe comes after 24 steps
  REPEAT
    r% = USR 60003
    n% = n% - 1: IF n% = 0 THEN new_pipe
    IF r% = 2 THEN point
  UNTIL r% = 1
ENDPROC
'
DEF PROC new_pipe
  ' Start the next pipe. Its gap is near the last one (g% moves
  ' up to 32 lines) and keeps its caps inside the playfield.
  ' Easy at first: the gap is 78 lines at score 0 and closes 2
  ' lines a point, down to 52 from score 13. The next pipe
  ' comes 32 steps later at first, one step sooner every 2
  ' points, down to 24 steps from score 16.
  gap% = MAX(52, 78 - 2 * score%)
  n% = MAX(24, 32 - (score% >> 1))
  g% = g% + 8 * RND(9) - 40
  g% = MAX(16, MIN(g%, 144 - gap%))
  POKE 60010, g%
  POKE 60011, g% + gap%
  POKE 60012, 1
ENDPROC
'
DEF PROC point
  ' The bat got past a pipe: one more point.
  score% = score% + 1
  PRINT AT 0, 30; PAPER 1; INK 7; BRIGHT 1; score%;
  BEEP 0.01, 48
ENDPROC
'
DEF PROC game_over
  ' The bat turns red and a falling tune plays. A new best score
  ' is kept in best%, which lasts until the program is run
  ' again. Then SPACE starts the next game.
  LOCAL n%, r%
  CALL 60006
  FOR n% = 0 TO 4
    BEEP 0.06, 12 - n% * 5
  NEXT
  FOR r% = 7 TO 14
    PRINT AT r%, 24; PAPER 2; INK 7; BRIGHT 1; box$;
  NEXT
  PRINT AT 8, 29; PAPER 2; INK 6; BRIGHT 1; "G A M E    O V E R";
  PRINT AT 10, 31; PAPER 2; INK 7; BRIGHT 1; "Score  "; score%;
  IF score% > best%
    best% = score%
    PRINT AT 11, 31; PAPER 2; INK 6; BRIGHT 1; FLASH 1; "A new best!";
  ELSE
    PRINT AT 11, 31; PAPER 2; INK 7; BRIGHT 1; "Best   "; best%;
  ENDIF
  PRINT AT 13, 27; PAPER 2; INK 5; BRIGHT 1; "SPACE to fly again";
  header
  footer
  WAIT 40
  release
  REPEAT
    WAIT 1
  UNTIL KEY(" ")
ENDPROC
