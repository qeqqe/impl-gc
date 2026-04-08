// Tests: INVOKEVIRTUAL, IRETURN, instance method calls, GETFIELD/PUTFIELD on int fields
public class Counter {
    public int count;

    public Counter() {
        this.count = 0;
    }

    public void increment() {
        this.count = this.count + 1;
    }

    public void add(int n) {
        this.count = this.count + n;
    }

    public int get() {
        return this.count;
    }

    public void reset() {
        this.count = 0;
    }
}
