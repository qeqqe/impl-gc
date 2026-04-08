// NEW, GETFIELD, PUTFIELD, INVOKESPECIAL (<init>), ALOAD, ASTORE
public class Node {
    public int value;
    public Node next;   // <- reference field, write barrier runs here

    public Node(int value) {
        this.value = value;
        this.next = null;
    }
}
