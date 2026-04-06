import os   

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day4.txt")) as f:  
    input = f.read()

DIR = [
    (-1, 0),
    (0, -1),
    (0, 1),
    (1, 0),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1)
]

lines = input.splitlines()

inbounds = lambda x, y: 0 <= x < len(lines[0]) and 0 <= y < len(lines)

start = []

for y, line in enumerate(lines):
    for x, ch in enumerate(line):
        if ch == "@":
            start.append((x, y))

def count_rolls(xy: (int, int)) -> int:
    x, y = xy

    out = 0

    for dx, dy in DIR:
        nx, ny = x + dx, y + dy
        if inbounds(nx, ny):
            out += int(lines[ny][nx] == '@')

    return int(out < 4)

out = sum(map(count_rolls, start))
print(out)