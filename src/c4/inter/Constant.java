package inter;

import lexer.*;
import treewalker.TreeWalker;


/*
 * Constant ist eine Unterklasse von Singleton und beschreibt Konstante. 
 * Die beiden Konstanten True und False sind hier als Klassenvariablen
 * definiert.
 */

public class Constant extends Singleton {

	public Constant(Token tok, Type p) {
		super(tok, p);
	}

	public Constant(int i) {
		super(new Num(i), Type.Int);
	}

	public boolean isConstant() {
		return true;
	}
	
	public static final Constant True = new Constant(Word.True, Type.Bool),
			False = new Constant(Word.False, Type.Bool);

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkConstantNode(this, arg);
	}

}
