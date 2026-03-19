package inter;

import lexer.*;

import treewalker.TreeWalker;

/*
 * Not ist eine Unterklasse von Logical und beschreibt 
 * die logische nicht-Operation. Beide von Logical geerbten
 * Instanzenvariablen werden auf den gleichen Ausdruck gesetzt. 
 */

public class Not extends Logical {

	public Not(Token tok, Expr x2) {
		super(tok, x2, x2);
	}
	
	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkNotNode(this, arg);
	}

}
