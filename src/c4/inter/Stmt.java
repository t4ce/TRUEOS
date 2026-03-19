package inter;

/*
 * Die Unterklasse Stmt von Node fasst alle Statements zusammen
 */

public abstract class Stmt extends Node {
	int next = 0; // Labelnummer für das next-Attribut
	
	private static Stmt enclosing = null;
	
	public Stmt() {};
	
	public int getNext() {
		return next;
	}

	public void setNext(int next) {
		this.next = next;
	}


	//  für das break-Statenment
	
	public static Stmt getEnclosing() {
		return enclosing;
	}

	public static void setEnclosing(Stmt enclosing) {
		Stmt.enclosing = enclosing;
	};

}
