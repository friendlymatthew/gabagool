import os                                                                  

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day1.txt")) as f:  
    input = f.read()

lines = input.splitlines()

out = 0
dial = 50

for line in lines:
    direction, clicks = line[0], int(line[1:])

    if direction == 'L':
        for _ in range(clicks):
            dial = (dial - 1) % 100
            if dial == 0:
                out += 1
    else:
        for _ in range(clicks):
            dial = (dial + 1) % 100
            if dial == 0:
                out += 1


# 6
print(out)