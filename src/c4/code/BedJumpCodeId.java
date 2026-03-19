package code;

import inter.Singleton;

/*
 * Diese Klasse definiert bedingte Sprünge der Form if(id) goto label
 */


public class BedJumpCodeId extends JumpCode {
	Singleton ident;
	
	public BedJumpCodeId(Singleton id, int i)
	{
		ident = id;
		label = i;
	}
		
	public String toString()
	{
		return ("if " + ident.toString() + " goto " + label);
	}
	
	

}
