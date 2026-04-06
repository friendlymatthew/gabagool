import os                                                                  

dir = os.path.dirname(__file__)                                            
with open(os.path.join(dir, "day3.txt")) as f:  
    input = f.read()

lines = input.splitlines()

def largest(line: str) -> int:
    stack = []

    for i, ch in enumerate(line):
        n = int(ch)
        
        while stack and stack[-1] < n:
            if len(line) - i + len(stack) > 12:
                stack.pop()
            else:
                break
        
        stack.append(n)
    
   
    out = int(''.join([str(ch) for ch in stack[:12]]))
    return out 

out = sum(map(largest, lines))
print(out)