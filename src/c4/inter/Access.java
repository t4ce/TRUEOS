package inter;
import treewalker.TreeWalker;
import code.ArrayRefCode;
import code.DreiAdrCode;

import lexer.*;

/*
 * Access ist eine Unterklasse von Op und beschreibt einen Array-Zugriff. 
 * In den Instanzenvariablen array und index werden die beiden Operanden 
 * abgelegt. Die Instanzenvariablen enthalten den Ausdruck für das Array
 * und für den Index.
 */

public class Access extends Op {
	Expr array;
	Expr index;

	public Access(Expr a, Expr i) {
		super(new Word("[]", Tag.INDEX));
		array = a;
		index = i;
	}

	// Für TranformWalker
	public Access(Expr a, Expr i, Type p) {
		super(new Word("[]", Tag.INDEX));
		array = a;
		index = i;
		type = p;
	}

	public Expr getArray() {
		return array;
	}

	public Expr getIndex() {
		return index;
	}
	
	public void setIndex(Expr index) {
		this.index = index;
	}

	// für die Drei-Adress-Code Erzeugung
	public DreiAdrCode codeForValueTo(Id id) {
		return (new ArrayRefCode(id, this));
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkAccessNode(this, arg);
	}


}
