package inter;

import lexer.*;
import treewalker.TreeWalker;

/*
 * Id ist eine Unterklasse von Singleton und beschreibt Identifier. 
 */

public class Id extends Singleton {
	int offset;					// offset gibt die relative Speicheradresse des Identifiers an
	
	public Id(Word id, Type p, int b) {
		super(id, p);
		offset = b;
	}

	public int getOffset() {
		return offset;
	}


	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkIdNode(this, arg);
	}

}
