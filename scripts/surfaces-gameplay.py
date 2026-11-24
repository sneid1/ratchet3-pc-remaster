"""Gameplay RAM dumps of every story level for PLAN 9.2 step 1's surface counts (`uya native --surfaces`), from the game itself.

    python -I scripts/surfaces-gameplay.py <out dir> [--levels 1,2,...]

Each level is loaded straight in (`uya play --start N --aspect 16:9` under scripts/uyactl.py: on the second monitor, which
is portrait here, at the 16:9 shape the player plays at, with the presses scripts/level-times.ps1
gives each level to reach play: Start at field 15, Cross at 420 on the vid-comics). A battle level (12, 18, 20, 26, 29) opens on
its BATTLE MISSIONS menu (level 12 after its skydive), whose screen counts as a pause screen: the harness waits for the menu and
starts the first mission, with Cross twice (18, 26) or a click on its row (12, 20, 29: `MISSION_ROW`). Past the level's load, gameplay is made sure of by the game itself: the pause screens open only in play,
so the harness runs 300 fields, presses Start and waits for the pause screens, again until they open (a Start during the
arrival cutscene or a radio message skips it or is not taken); then it closes them, runs 120 fields and has the control
channel write the next field's RAM and jobs (`dump DIR`) and its picture (`shot`) into a folder per level (fields repeat across
levels): <out dir>/LNN/ramFFFFF.bin, jobsFFFFF.bin and LNN-fFFFFF.png. scripts/surfaces-measure.ps1 -Dumps <out dir> then counts the surfaces on them.
"""
import argparse
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BATTLE = (12, 18, 20, 26, 29)
# The first mission's row on the BATTLE MISSIONS menu (share of the picture's height, 1920 x 1080) where Cross on the first row
# does not start it: level 29's first row is "Return to your ship"; levels 12 and 20 took no Cross. Levels 18 and 26 start on
# Cross, Cross.
MISSION_ROW = {12: 0.667, 20: 0.667, 29: 0.711}
LEVELS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36]


# The camera turned about a quarter: the mouse moved 40 counts right a field for this many fields (Veldin: 45 fields turned it
# about a third; the right stick held, `look`, did not turn it there).
TURN = 30


def level(n, out, views=0):
    press = '15:800:12'
    if 31 <= n <= 35:
        press += ',420:40:4'
    p = subprocess.Popen([sys.executable, '-I', os.path.join(ROOT, 'scripts', 'uyactl.py'), '--', '--start', str(n), '--press', press, '--aspect', '16:9'],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1, encoding='utf-8', errors='replace')

    def cmd(c):
        p.stdin.write(c + '\n')
        p.stdin.flush()
        for line in p.stdout:
            line = line.rstrip('\n')
            if ' -> ' in line:
                ans = line.split(' -> ', 1)[1]
                print(f'  L{n:02d} {c:28s} -> {ans}', flush=True)
                return ans
            print(f'  L{n:02d} {line}', flush=True)
        raise SystemExit(f'level {n}: the game ended at "{c}"')

    try:
        if not cmd(f'until level {n} max 30000').startswith('ok'):
            return f'level {n}: never drawn'
        if n in BATTLE:
            # The menu comes up after the level's fly-in (level 20: none at field 300 after the level, up at 900); `until menu`
            # holds from the level's start, so the harness waits the fields out.
            cmd('run 900')
            if n in MISSION_ROW:
                # A click on the mission's row starts it (the menus take the mouse: PLAN, "Every level's menus with the mouse").
                cmd(f'pointer 0.5 {MISSION_ROW[n]} click')
                cmd('run 30')
            else:
                cmd('press 40 4')
                cmd('run 200')
                cmd('press 40 4')
        # A mission can end fast on a fresh save (level 20's back on its menu 1,800 fields after its start): short steps there.
        step = 120 if n in BATTLE else 300
        for _ in range(40):
            cmd(f'run {step}')
            cmd('press 800 4')
            if cmd('until pause max 240').startswith('ok'):
                break
        else:
            return f'level {n}: no pause screen after 40 tries: no gameplay reached'
        cmd('press 800 4')
        cmd('run 120')
        d = os.path.join(out, f'L{n:02d}')

        def dump():
            ans = cmd(f'dump {d}')
            field = int(ans.split('ok field ')[1].split("'")[0])
            cmd('run 1')
            cmd(f'shot {d}/L{n:02d}-f{field:05d}.png')
            return field

        fields = [dump()]
        # --views: more of the level for its objects (PLAN 9.2: every prop, shrub and enemy measured where it stands): the camera
        # turned a quarter at a time where Ratchet arrived (the mouse), then Ratchet walked forward and the turns
        # again; a dump after each.
        for walk in range(views // 4):
            if walk:
                cmd('stick 0 1 240')
                cmd('run 30')
            for _ in range(4 if walk or views < 4 else 3):
                cmd(f'mouse 40 0 {TURN}')
                cmd('run 20')
                fields.append(dump())
        return f'level {n}: gameplay dumps at fields {fields}'
    finally:
        try:
            cmd('quit')
        except (SystemExit, OSError):
            pass
        p.wait(timeout=60)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('out')
    ap.add_argument('--levels', default=','.join(map(str, LEVELS)))
    ap.add_argument('--views', type=int, default=0, help='more dumps per level: the camera turned a quarter at a time (4: one turn round, 8: then a walk and another)')
    a = ap.parse_args()
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    results = [level(int(n), out, a.views) for n in a.levels.split(',')]
    print('\n'.join(results))


if __name__ == '__main__':
    main()
