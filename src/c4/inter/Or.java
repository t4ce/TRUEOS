package inter;

import lexer.*;
import treewalker.TreeWalker;

/*
 * Or ist eine Unterklasse von Logical und beschreibt 
 * die logische oder-Operation
 */

public class Or extends Logical {

	public Or(Token tok, Expr x1, Expr x2) {
		super(tok, x1, x2);
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkOrNode(this, arg);
	}

}
