package inter;

import treewalker.TreeWalker;

/*
 * Programm ist die Klasse der Wurzelknoten der Syntaxbäume
 */
public class Program extends Stmt {
	Block block;

	public Program(Block b) {
		 block = b;
	}
		
	public Block getBlock() {
		return block;
	}

	public <R, P> R walk(TreeWalker<R, P> walker, P arg) {
		return walker.walkProgramNode(this, arg);
	}

}

	
