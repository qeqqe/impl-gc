// Master test class. Exercises every opcode category your interpreter implements.
public class GcStressTest {

    // ── Test 1: Arithmetic + back-edge loop ──────────────────────────────────
    // Opcodes: ICONST_0, ICONST_1, ISTORE, ILOAD, IADD, IF_ICMPGT, GOTO, IRETURN
    // Expected: sumTo(100) = 5050
    public static int sumTo(int n) {
        int sum = 0;
        int i = 1;
        while (i <= n) {
            sum = sum + i;
            i = i + 1;
        }
        return sum;
    }

    // ── Test 2: Iterative fibonacci ───────────────────────────────────────────
    // Opcodes: ISTORE, ILOAD, IADD, IINC, IF_ICMPGT, GOTO, IRETURN
    // Expected: fib(10) = 55, fib(20) = 6765
    public static int fibonacci(int n) {
        if (n <= 1) {
            return n;
        }
        int a = 0;
        int b = 1;
        int i = 2;
        while (i <= n) {
            int temp = a + b;
            a = b;
            b = temp;
            i = i + 1;
        }
        return b;
    }

    // ── Test 3: Multiplication table (IMUL, nested loops) ────────────────────
    // Opcodes: IMUL, nested GOTO back-edges (two safepoint poll sites)
    // Expected: product(5) = 1*1 + 1*2 + ... + 5*5 = 225
    public static int productSum(int n) {
        int total = 0;
        int i = 1;
        while (i <= n) {
            int j = 1;
            while (j <= n) {
                total = total + (i * j);
                j = j + 1;
            }
            i = i + 1;
        }
        return total;
    }

    // ── Test 4: Integer division + remainder ─────────────────────────────────
    // Opcodes: IDIV, IREM
    // Expected: collatz(27) steps = 111
    public static int collatz(int n) {
        int steps = 0;
        while (n != 1) {
            if (n % 2 == 0) {
                n = n / 2;
            } else {
                n = n * 3 + 1;
            }
            steps = steps + 1;
        }
        return steps;
    }

    // ── Test 5: Object allocation + int field access ──────────────────────────
    // Opcodes: NEW, DUP, INVOKESPECIAL, INVOKEVIRTUAL, GETFIELD, PUTFIELD (int)
    // Expected: counter.get() = 500 after 500 increments
    public static int testCounter() {
        Counter c = new Counter();
        int i = 0;
        while (i < 500) {
            c.increment();
            i = i + 1;
        }
        return c.get();
    }

    // ── Test 6: Linked list construction ─────────────────────────────────────
    // Opcodes: NEW, PUTFIELD (reference = WRITE BARRIER fires!), ASTORE, ALOAD
    // This allocates `size` Node objects — should trigger minor GC if size > TLAB
    // Expected: buildList(200) returns head of list 199->198->...->0
    public static Node buildList(int size) {
        Node head = null;
        int i = 0;
        while (i < size) {
            Node node = new Node(i);
            node.next = head;     // <-- WRITE BARRIER: node.next = reference
            head = node;
            i = i + 1;
        }
        return head;
    }

    // ── Test 7: Linked list traversal ────────────────────────────────────────
    // Opcodes: GETFIELD (int), GETFIELD (reference), IFNULL, ASTORE
    // Expected: sumList(buildList(200)) = 0+1+...+199 = 19900
    public static int sumList(Node head) {
        int total = 0;
        Node curr = head;
        while (curr != null) {
            total = total + curr.value;
            curr = curr.next;
        }
        return total;
    }

    // ── Test 8: GC pressure — build and drop many lists ───────────────────────
    // Forces multiple minor GC cycles. Nodes allocated in inner loop become
    // garbage when outer loop iteration ends.
    // Surviving `keeper` node tests promotion to old gen.
    public static int gcPressure() {
        Node keeper = new Node(9999);  // this should get promoted to old gen
        int rounds = 0;
        while (rounds < 10) {
            // allocate 100 nodes per round — total 1000 Node objects
            Node head = null;
            int i = 0;
            while (i < 100) {
                Node n = new Node(i);
                n.next = head;
                head = n;
                i = i + 1;
            }
            // head goes out of scope here — all 100 nodes are garbage
            // GC should reclaim them. keeper must survive.
            keeper.value = keeper.value + rounds;
            rounds = rounds + 1;
        }
        return keeper.value; // 9999 + 0+1+2+...+9 = 9999 + 45 = 10044
    }

    // ── Test 9: Counter with multiple method types ────────────────────────────
    // Tests INVOKEVIRTUAL with args (add(n)), reset(), chained calls
    // Expected: 100 + 200 + 50 - reset - 75 = 75
    public static int testCounterMethods() {
        Counter c = new Counter();
        c.add(100);
        c.add(200);
        c.add(50);
        c.reset();
        c.add(75);
        return c.get();  // 75
    }

    // ── Test 10: Mixed object graph (old→young pointer) ───────────────────────
    // Forces the write barrier + card table dirty path.
    // We create an "old" node, then assign a new node to its .next field.
    // In a real minor GC cycle, this cross-gen pointer must be caught by card table.
    public static int testCrossGenPointer() {
        // In a real GC: oldNode would be promoted first, then we'd assign
        // a young node to its .next — firing the write barrier.
        // Here we just verify the object graph is consistent.
        Node a = new Node(1);
        Node b = new Node(2);
        Node c = new Node(3);
        a.next = b;    // write barrier
        b.next = c;    // write barrier
        c.next = null;

        // traverse: 1 + 2 + 3 = 6
        return sumList(a);
    }

    public static void main(String[] args) {
        // Run all tests and store results in locals
        // (no System.out yet — check results in your debugger/assert layer)

        int t1 = sumTo(100);            // expect 5050
        int t2 = fibonacci(10);         // expect 55
        int t3 = fibonacci(20);         // expect 6765
        int t4 = productSum(5);         // expect 225
        int t5 = collatz(27);           // expect 111
        int t6 = testCounter();         // expect 500
        int t7 = sumList(buildList(200));  // expect 19900
        int t8 = gcPressure();          // expect 10044
        int t9 = testCounterMethods();  // expect 75
        int t10 = testCrossGenPointer(); // expect 6
    }
}
