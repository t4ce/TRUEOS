package inter;
import treewalker.TreeWalker;

import lexer.*;

/*
 * And ist eine Unterklasse von Logical und beschreibt 
 * die logische und-Operation
 */

public class And extends Logical {

	public And(Token tok, Expr x1, Expr x2) {
		super(tok, x1, x2);
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkAndNode(this, arg);
	}

}
