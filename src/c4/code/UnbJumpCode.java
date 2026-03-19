package code;

/*
 * Diese Klasse definiert unbedingte Sprünge der Form  goto label
 */


public class UnbJumpCode extends JumpCode {
	
	public UnbJumpCode(int i) {
		label = i;
	}
	
	
	public String toString()
	{
		return ("goto " + label);
	}

}
